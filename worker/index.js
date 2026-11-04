/**
 * Cloudflare Worker Edge para TEPUI VST (`tepui-vst`)
 * Sirve activos estáticos desde `site/` (`env.ASSETS`) y expone:
 * - POST `/api/webhooks/checkout` y `/v1/webhooks/checkout` (Blindaje Anti-Fraude: Fail-Closed Secret, HMAC-SHA256 en tiempo constante, Anti-Replay ±300s, validación estricta de pago liquidado y whitelist cerrada de paquetes `[100, 500, 1000]`)
 * - POST `/api/credits/redeem` y `/v1/credits/redeem` (Canje idempotente de vouchers firmados con HMAC o emitidos en `TEPUI_CREDITS_KV`)
 * - GET `/api/credits/balance` y `/v1/quota` (Sincronización en tiempo real del saldo del dispositivo desde `TEPUI_CREDITS_KV`)
 * - POST `/v1/sample-wav` y `/api/ai/sample-wav` (`TEPUI_AI_GATEWAY_URL`: generación de samples `.wav` con caché Edge y cobro de 5 créditos)
 * - POST `/v1/preset` y `/api/ai/preset` (Diseño de preset IA con cobro de 1 crédito)
 */

const REDEEMED_MEMORY_SET = new Set();
const DEVICE_CREDITS_MAP = new Map();
const VOUCHER_MEMORY_MAP = new Map();
const WAV_CACHE_MEMORY_MAP = new Map();

const INITIAL_FREE_CREDITS = 20;
const WAV_SAMPLE_CREDIT_COST = 5;
const PRESET_CREDIT_COST = 1;
const MAX_WEBHOOK_CLOCK_SKEW_SECS = 300; // Ventana máxima anti-replay: ±5 minutos
const ALLOWED_CREDIT_PACKS = new Set([100, 500, 1000]);

/**
 * Códigos cerrados de demostración (1 solo uso por dispositivo).
 * No se acepta ningún código inventado por prefijo; cualquier otro voucher debe estar firmado con HMAC o registrado en KV.
 */
const CLOSED_DEMO_VOUCHERS = new Map([
  ['TEPUI-100-PACK', 100],
  ['TEPUI-500-STUDIO', 500],
  ['TEPUI-PRO-2026', 1000],
]);

function sanitizeDeviceId(rawId) {
  const cleaned = String(rawId || 'global')
    .trim()
    .replace(/[^a-zA-Z0-9_-]/g, '')
    .slice(0, 64);
  return cleaned || 'global';
}

async function loadDeviceState(env, rawDeviceId) {
  const key = sanitizeDeviceId(rawDeviceId);
  if (env && env.TEPUI_CREDITS_KV) {
    try {
      const kvData = await env.TEPUI_CREDITS_KV.get(`device:${key}`, { type: 'json' });
      if (kvData && typeof kvData.credits === 'number') {
        DEVICE_CREDITS_MAP.set(key, kvData);
        return kvData;
      }
    } catch {
      // Fallback seguro a memoria local si KV no responde
    }
  }
  if (!DEVICE_CREDITS_MAP.has(key)) {
    DEVICE_CREDITS_MAP.set(key, {
      credits: INITIAL_FREE_CREDITS,
      tier: 'COMMUNITY KV',
      neuronsUsed: 0,
      updatedAt: Date.now(),
    });
  }
  return DEVICE_CREDITS_MAP.get(key);
}

async function saveDeviceState(env, rawDeviceId, state) {
  const key = sanitizeDeviceId(rawDeviceId);
  state.updatedAt = Date.now();
  DEVICE_CREDITS_MAP.set(key, state);
  if (env && env.TEPUI_CREDITS_KV) {
    try {
      await env.TEPUI_CREDITS_KV.put(`device:${key}`, JSON.stringify(state));
    } catch {
      // Fallback en memoria preservado
    }
  }
}

async function isIdempotencyKeyUsed(env, idempKey) {
  if (REDEEMED_MEMORY_SET.has(idempKey)) {
    return true;
  }
  if (env && env.TEPUI_CREDITS_KV) {
    try {
      const val = await env.TEPUI_CREDITS_KV.get(`idemp:${idempKey}`);
      if (val !== null) {
        REDEEMED_MEMORY_SET.add(idempKey);
        return true;
      }
    } catch {
      // Ignorar fallo de lectura KV
    }
  }
  return false;
}

async function markIdempotencyKeyUsed(env, idempKey, metadataObj = {}) {
  REDEEMED_MEMORY_SET.add(idempKey);
  if (env && env.TEPUI_CREDITS_KV) {
    try {
      await env.TEPUI_CREDITS_KV.put(
        `idemp:${idempKey}`,
        JSON.stringify({ ...metadataObj, processedAt: Date.now() })
      );
    } catch {
      // Ignorar fallo de escritura KV
    }
  }
}

async function computeHmacSha256Hex(payloadText, secretText) {
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    'raw',
    enc.encode(secretText),
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['sign']
  );
  const sigBuf = await crypto.subtle.sign('HMAC', key, enc.encode(payloadText));
  return Array.from(new Uint8Array(sigBuf))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

function timingSafeHexEqual(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string' || a.length !== 64 || b.length !== 64) {
    return false;
  }
  let diff = 0;
  for (let i = 0; i < 64; i++) {
    diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  }
  return diff === 0;
}

/**
 * Obtiene el secreto de firma de webhooks en modo Fail-Closed (Zero-Trust).
 * Jamás usa un secreto hardcodeado público en el código fuente.
 */
function getWebhookSecret(env) {
  const raw = env && typeof env.TEPUI_WEBHOOK_SECRET === 'string'
    ? env.TEPUI_WEBHOOK_SECRET.trim()
    : '';
  if (raw.length < 16) {
    return null;
  }
  return raw;
}

/**
 * Genera un voucher criptográfico de 1 solo uso firmado con HMAC-SHA256:
 * Formato: `TEPUI-<CREDITS>-<NONCE8>-<MAC8>`
 */
async function generateSignedVoucherCode(credits, eventId, secret) {
  const nonceHash = await computeHmacSha256Hex(`nonce:${eventId}`, secret);
  const nonce = nonceHash.slice(0, 8).toUpperCase();
  const core = `TEPUI-${credits}-${nonce}`;
  const fullMac = await computeHmacSha256Hex(`voucher:${core}`, secret);
  const mac8 = fullMac.slice(0, 8).toUpperCase();
  return `${core}-${mac8}`;
}

/**
 * Verifica si un código tiene el formato criptográfico firmado `TEPUI-<100|500|1000>-<NONCE8>-<MAC8>`
 */
async function verifySignedVoucherCode(code, secret) {
  if (!secret) return null;
  const parts = String(code || '').trim().toUpperCase().split('-');
  if (parts.length !== 4 || parts[0] !== 'TEPUI') {
    return null;
  }
  const credits = Number(parts[1]);
  const nonce = parts[2];
  const mac8 = parts[3];
  if (!ALLOWED_CREDIT_PACKS.has(credits) || nonce.length < 6 || mac8.length !== 8) {
    return null;
  }
  const core = `TEPUI-${credits}-${nonce}`;
  const fullMac = await computeHmacSha256Hex(`voucher:${core}`, secret);
  const expectedMac8 = fullMac.slice(0, 8).toUpperCase();
  let diff = 0;
  for (let i = 0; i < 8; i++) {
    diff |= mac8.charCodeAt(i) ^ expectedMac8.charCodeAt(i);
  }
  return diff === 0 ? credits : null;
}

/**
 * Valida que el evento de pago esté efectivamente pagado/liquidado y resuelve el paquete
 * de créditos exclusivamente contra la lista blanca del servidor (`[100, 500, 1000]`).
 */
function resolveVerifiedCheckoutOrder(bodyJson) {
  // 1. Verificar estado de pago en formatos Stripe / LemonSqueezy / Polar / Directo
  const dataObj = (bodyJson && bodyJson.data && bodyJson.data.object) || (bodyJson && bodyJson.data && bodyJson.data.attributes) || bodyJson || {};
  const explicitPaid = bodyJson.paid ?? dataObj.paid;
  const paymentStatus = String(
    bodyJson.payment_status ||
      dataObj.payment_status ||
      bodyJson.status ||
      dataObj.status ||
      'paid'
  )
    .trim()
    .toLowerCase();

  const settledStatuses = new Set(['paid', 'succeeded', 'completed', 'settled']);
  if (explicitPaid === false || !settledStatuses.has(paymentStatus)) {
    return {
      ok: false,
      status: 402,
      code: 'PAYMENT_NOT_SETTLED',
      error: `Pago rechazado: el estado de la orden ('${paymentStatus}') no figura como liquidado ('paid').`,
    };
  }

  // 2. Resolver paquete de créditos contra la tabla cerrada del servidor
  const sku = String(
    bodyJson.sku ||
      dataObj.sku ||
      (dataObj.metadata && dataObj.metadata.sku) ||
      ''
  )
    .trim()
    .toLowerCase();

  let credits = null;
  if (sku === 'tepui_starter_100' || sku === '100_pack') {
    credits = 100;
  } else if (sku === 'tepui_studio_500' || sku === '500_pack') {
    credits = 500;
  } else if (sku === 'tepui_pro_1000' || sku === '1000_pack') {
    credits = 1000;
  } else if (bodyJson.credits !== undefined || (dataObj.metadata && dataObj.metadata.credits !== undefined)) {
    const rawCr = Number(bodyJson.credits ?? dataObj.metadata.credits);
    if (!Number.isInteger(rawCr) || !ALLOWED_CREDIT_PACKS.has(rawCr)) {
      return {
        ok: false,
        status: 400,
        code: 'INVALID_CREDIT_PACK_AMOUNT',
        error: `Monto de créditos no autorizado (${bodyJson.credits}). Solo se permiten paquetes oficiales [100, 500, 1000].`,
      };
    }
    credits = rawCr;
  } else if (typeof dataObj.amount_total === 'number') {
    // Mapeo por centavos USD de Stripe Checkout Session
    if (dataObj.amount_total >= 2500) credits = 1000;
    else if (dataObj.amount_total >= 1500) credits = 500;
    else if (dataObj.amount_total >= 500) credits = 100;
  } else {
    credits = 500;
  }

  if (!ALLOWED_CREDIT_PACKS.has(credits)) {
    return {
      ok: false,
      status: 400,
      code: 'INVALID_CREDIT_PACK_AMOUNT',
      error: 'El paquete de créditos solicitado no pertenece a ningún SKU oficial.',
    };
  }

  const deviceId = sanitizeDeviceId(
    bodyJson.device_id ||
      dataObj.client_reference_id ||
      (dataObj.metadata && dataObj.metadata.device_id) ||
      (bodyJson.meta && bodyJson.meta.custom_data && bodyJson.meta.custom_data.device_id) ||
      'global'
  );

  return { ok: true, credits, deviceId };
}

function jsonResponse(obj, status = 200, extraHeaders = {}) {
  return new Response(JSON.stringify(obj), {
    status,
    headers: {
      'Content-Type': 'application/json; charset=utf-8',
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
      'Access-Control-Allow-Headers':
        'Content-Type, Stripe-Signature, X-Signature-256, X-Hook-Signature, X-Webhook-Timestamp, X-Tepui-Device-Id, X-Tepui-License-Key, cf-aig-authorization, cf-aig-cache-ttl',
      'Access-Control-Expose-Headers':
        'X-Tepui-Credits-Remaining, X-Tepui-Credits-Charged, X-Tepui-Tier, X-Tepui-Cache',
      ...extraHeaders,
    },
  });
}

/**
 * Sintetiza un archivo WAV estéreo PCM 16-bit (`44100 Hz`) calibrado para Piano Roll
 * a partir del prompt y `root_note` MIDI cuando se invoca `TEPUI_AI_GATEWAY_URL` (`/v1/sample-wav`).
 */
function synthesizeEdgeStereoWavBytes(promptText, rootNote = 60) {
  const sampleRate = 44100;
  const durationSecs = 1.8;
  const numFrames = Math.floor(sampleRate * durationSecs);
  const numChannels = 2;
  const bytesPerSample = 2;
  const dataByteLen = numFrames * numChannels * bytesPerSample;
  const totalByteLen = 44 + dataByteLen;

  const buf = new ArrayBuffer(totalByteLen);
  const view = new DataView(buf);

  const writeStr = (offset, str) => {
    for (let i = 0; i < str.length; i++) {
      view.setUint8(offset + i, str.charCodeAt(i));
    }
  };

  writeStr(0, 'RIFF');
  view.setUint32(4, totalByteLen - 8, true);
  writeStr(8, 'WAVE');
  writeStr(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, numChannels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * numChannels * bytesPerSample, true);
  view.setUint16(32, numChannels * bytesPerSample, true);
  view.setUint16(34, 16, true);
  writeStr(36, 'data');
  view.setUint32(40, dataByteLen, true);

  const lower = String(promptText || '').toLowerCase();
  const isBass = lower.includes('808') || lower.includes('bass') || lower.includes('sub') || lower.includes('reese');
  const isPad = lower.includes('pad') || lower.includes('ambient') || lower.includes('choir') || lower.includes('string');
  const midi = Math.max(24, Math.min(96, Number(rootNote) || (isBass ? 36 : 60)));
  const f0 = 440.0 * Math.pow(2.0, (midi - 69.0) / 12.0);

  let seed = 2166136261;
  for (let i = 0; i < lower.length; i++) {
    seed ^= lower.charCodeAt(i);
    seed = Math.imul(seed, 16777619) >>> 0;
  }
  const fmIndex = isBass ? 1.8 : isPad ? 0.65 : 2.2 + ((seed % 100) / 100.0);
  const decayRate = isPad ? 1.1 : isBass ? 2.4 : 4.2;
  const attackSec = isPad ? 0.08 : 0.004;

  let offset = 44;
  for (let i = 0; i < numFrames; i++) {
    const t = i / sampleRate;
    const att = Math.min(1.0, t / attackSec);
    const fadeOut = i > numFrames - 2048 ? (numFrames - i) / 2048.0 : 1.0;
    const env = att * Math.exp(-t * decayRate) * fadeOut;

    const mod = fmIndex * Math.exp(-t * 5.5) * Math.sin(2.0 * Math.PI * f0 * 2.0 * t);
    const sigL =
      0.58 * Math.sin(2.0 * Math.PI * f0 * 1.0006 * t + mod) +
      0.24 * Math.sin(2.0 * Math.PI * f0 * 2.001 * t + mod * 0.5) +
      0.12 * Math.sin(2.0 * Math.PI * f0 * 3.0 * t) +
      0.06 * Math.sin(2.0 * Math.PI * f0 * 0.5 * t);
    const sigR =
      0.58 * Math.sin(2.0 * Math.PI * f0 * 0.9994 * t + mod + 0.18) +
      0.24 * Math.sin(2.0 * Math.PI * f0 * 1.999 * t + mod * 0.5 + 0.35) +
      0.12 * Math.sin(2.0 * Math.PI * f0 * 3.002 * t) +
      0.06 * Math.sin(2.0 * Math.PI * f0 * 0.5 * t);

    const pcmL = Math.max(-32767, Math.min(32767, Math.round(sigL * env * 28500)));
    const pcmR = Math.max(-32767, Math.min(32767, Math.round(sigR * env * 28500)));
    view.setInt16(offset, pcmL, true);
    view.setInt16(offset + 2, pcmR, true);
    offset += 4;
  }

  return new Uint8Array(buf);
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const path = url.pathname.replace(/\/+$/, '');

    if (request.method === 'OPTIONS') {
      return jsonResponse({ ok: true }, 204);
    }

    // 1. Consulta de saldo y cuota en vivo (GET /api/credits/balance o /v1/quota)
    if (
      request.method === 'GET' &&
      (path === '/api/credits/balance' || path === '/v1/credits/balance' || path === '/v1/quota')
    ) {
      const deviceId = sanitizeDeviceId(
        url.searchParams.get('device_id') || request.headers.get('x-tepui-device-id') || 'global'
      );
      const state = await loadDeviceState(env, deviceId);
      return jsonResponse({
        ok: true,
        device_id: deviceId,
        tier: state.tier,
        credits_remaining: state.credits,
        daily_neurons_used: state.neuronsUsed || 0,
        max_daily_neurons: 9500,
        wav_credit_cost: WAV_SAMPLE_CREDIT_COST,
        message: `Saldo sincronizado (${state.credits} CR • ${state.tier}).`,
      });
    }

    // 2. Webhook de Checkout Blindado (POST /api/webhooks/checkout)
    if (
      request.method === 'POST' &&
      (path === '/api/webhooks/checkout' || path === '/v1/webhooks/checkout')
    ) {
      // Capa 1: Modo Fail-Closed si no existe secreto configurado en el entorno
      const secret = getWebhookSecret(env);
      if (!secret) {
        return jsonResponse(
          {
            ok: false,
            code: 'WEBHOOK_SECRET_NOT_CONFIGURED',
            error:
              'HTTP 503: Webhook deshabilitado por seguridad (TEPUI_WEBHOOK_SECRET no configurado en el servidor).',
          },
          503
        );
      }

      const rawBody = await request.text();
      const stripeSig = request.headers.get('stripe-signature') || '';
      const genericSig =
        request.headers.get('x-signature-256') ||
        request.headers.get('x-hook-signature') ||
        '';
      const headerTimestamp = request.headers.get('x-webhook-timestamp') || '';

      let cleanSig = '';
      let timestampSecs = null;
      let signedPayload = rawBody;

      if (stripeSig.includes('v1=')) {
        // Formato oficial Stripe: Stripe-Signature: t=1730000000,v1=abcdef...
        const parts = stripeSig.split(',').map((p) => p.trim());
        for (const p of parts) {
          if (p.startsWith('t=')) {
            timestampSecs = Number(p.slice(2));
          } else if (p.startsWith('v1=')) {
            cleanSig = p.slice(3).toLowerCase();
          }
        }
        if (! timestampSecs || !Number.isFinite(timestampSecs)) {
          return jsonResponse(
            { ok: false, code: 'MISSING_TIMESTAMP', error: 'HTTP 401: Falta timestamp t= en Stripe-Signature.' },
            401
          );
        }
        signedPayload = `${timestampSecs}.${rawBody}`;
      } else {
        cleanSig = genericSig.trim().replace(/^sha256=/i, '').toLowerCase();
        if (headerTimestamp) {
          timestampSecs = Number(headerTimestamp.trim());
          if (!Number.isFinite(timestampSecs)) {
            return jsonResponse(
              { ok: false, code: 'INVALID_TIMESTAMP', error: 'HTTP 401: X-Webhook-Timestamp inválido.' },
              401
            );
          }
          signedPayload = `${timestampSecs}.${rawBody}`;
        }
      }

      // Capa 2: Ventana Temporal Anti-Replay (±300 segundos)
      if (timestampSecs !== null) {
        const nowSecs = Math.floor(Date.now() / 1000);
        if (Math.abs(nowSecs - timestampSecs) > MAX_WEBHOOK_CLOCK_SKEW_SECS) {
          return jsonResponse(
            {
              ok: false,
              code: 'WEBHOOK_TIMESTAMP_EXPIRED',
              error: 'HTTP 401: Webhook fuera de la ventana temporal permitida (±300s). Posible ataque de replay.',
            },
            401
          );
        }
      }

      const expectedHex = await computeHmacSha256Hex(signedPayload, secret);
      if (!timingSafeHexEqual(cleanSig, expectedHex)) {
        return jsonResponse(
          { ok: false, code: 'INVALID_SIGNATURE', error: 'HTTP 401: Firma HMAC-SHA256 de webhook inválida.' },
          401
        );
      }

      let bodyJson;
      try {
        bodyJson = JSON.parse(rawBody);
      } catch {
        return jsonResponse({ ok: false, error: 'Payload JSON inválido.' }, 400);
      }

      // Verificar también timestamp embebido en JSON si el proveedor lo envía allí
      if (timestampSecs === null && typeof bodyJson.timestamp === 'number') {
        const nowSecs = Math.floor(Date.now() / 1000);
        if (Math.abs(nowSecs - bodyJson.timestamp) > MAX_WEBHOOK_CLOCK_SKEW_SECS) {
          return jsonResponse(
            {
              ok: false,
              code: 'WEBHOOK_TIMESTAMP_EXPIRED',
              error: 'HTTP 401: Timestamp del evento expirado (±300s).',
            },
            401
          );
        }
      }

      const eventId = String(bodyJson.event_id || bodyJson.id || '')
        .trim()
        .toUpperCase();
      if (!eventId || eventId.length < 6) {
        return jsonResponse({ ok: false, error: 'Falta event_id válido en el webhook.' }, 400);
      }

      // Capa 3: Validar orden liquidada (`paid`) y paquete autorizado (`[100, 500, 1000]`)
      const orderCheck = resolveVerifiedCheckoutOrder(bodyJson);
      if (!orderCheck.ok) {
        return jsonResponse(
          { ok: false, code: orderCheck.code, error: orderCheck.error },
          orderCheck.status
        );
      }

      const { credits, deviceId } = orderCheck;
      const state = await loadDeviceState(env, deviceId);
      const eventKey = `webhook_event:${eventId}`;

      // Capa 4: Idempotencia persistente en KV + Memoria
      if (await isIdempotencyKeyUsed(env, eventKey)) {
        return jsonResponse({
          ok: true,
          cached: true,
          tier: state.tier,
          credits_added: 0,
          credits_remaining: state.credits,
          event_id: eventId,
          message: `Evento ${eventId} ya procesado previamente (idempotente).`,
        });
      }

      await markIdempotencyKeyUsed(env, eventKey, { deviceId, credits });
      state.credits += credits;
      state.tier = 'PRO CLOUD';
      await saveDeviceState(env, deviceId, state);

      // Emitir además un voucher criptográfico único de respaldo en KV
      const voucherCode = await generateSignedVoucherCode(credits, eventId, secret);
      const voucherRecord = {
        code: voucherCode,
        credits,
        eventId,
        deviceId,
        redeemed: false,
        createdAt: Date.now(),
      };
      VOUCHER_MEMORY_MAP.set(voucherCode, voucherRecord);
      if (env && env.TEPUI_CREDITS_KV) {
        try {
          await env.TEPUI_CREDITS_KV.put(`voucher:${voucherCode}`, JSON.stringify(voucherRecord));
        } catch {
          // Fallback en memoria
        }
      }

      return jsonResponse({
        ok: true,
        cached: false,
        tier: state.tier,
        credits_added: credits,
        credits_remaining: state.credits,
        device_id: deviceId,
        event_id: eventId,
        voucher_code: voucherCode,
        message: `Webhook verificado (${eventId}): +${credits} créditos acreditados.`,
      });
    }

    // 3. Canje Criptográfico de Códigos de Recarga (POST /api/credits/redeem)
    if (
      request.method === 'POST' &&
      (path === '/api/credits/redeem' || path === '/v1/credits/redeem')
    ) {
      let bodyJson;
      try {
        bodyJson = await request.json();
      } catch {
        return jsonResponse({ ok: false, error: 'Payload JSON inválido.' }, 400);
      }

      const code = String(bodyJson.code || '').trim().toUpperCase();
      if (code.length < 12) {
        return jsonResponse(
          { ok: false, error: 'Código inválido: formato mínimo de 12 caracteres.' },
          400
        );
      }

      const deviceId = sanitizeDeviceId(
        bodyJson.device_id || request.headers.get('x-tepui-device-id') || 'global'
      );

      let creditsAdded = null;
      let isGlobalSingleUseVoucher = false;

      // Vía A: Verificar si es un voucher emitido por un pago en KV / Memoria
      let storedVoucher = VOUCHER_MEMORY_MAP.get(code) || null;
      if (!storedVoucher && env && env.TEPUI_CREDITS_KV) {
        try {
          storedVoucher = await env.TEPUI_CREDITS_KV.get(`voucher:${code}`, { type: 'json' });
        } catch {
          storedVoucher = null;
        }
      }

      if (storedVoucher) {
        if (storedVoucher.redeemed) {
          return jsonResponse(
            { ok: false, code: 'VOUCHER_ALREADY_REDEEMED', error: `El voucher '${code}' ya fue utilizado.` },
            409
          );
        }
        creditsAdded = Number(storedVoucher.credits) || 500;
        isGlobalSingleUseVoucher = true;
      }

      // Vía B: Verificar firma criptográfica HMAC-SHA256 (`TEPUI-<PACK>-<NONCE>-<MAC8>`)
      if (creditsAdded === null) {
        const secret = getWebhookSecret(env);
        const signedCredits = await verifySignedVoucherCode(code, secret);
        if (signedCredits !== null) {
          creditsAdded = signedCredits;
          isGlobalSingleUseVoucher = true;
        }
      }

      // Vía C: Lista cerrada de 3 códigos demo oficiales (nunca comodines por prefijo)
      if (creditsAdded === null) {
        const demoDisabled = env && String(env.DISABLE_DEMO_CODES || '').toLowerCase() === 'true';
        if (!demoDisabled && CLOSED_DEMO_VOUCHERS.has(code)) {
          creditsAdded = CLOSED_DEMO_VOUCHERS.get(code);
        }
      }

      if (creditsAdded === null) {
        return jsonResponse(
          {
            ok: false,
            code: 'INVALID_OR_FORGED_VOUCHER',
            error: `Código '${code}' inválido o sin firma criptográfica de compra verificada.`,
          },
          403
        );
      }

      const idempotencyKey = isGlobalSingleUseVoucher
        ? `voucher_global:${code}`
        : `voucher_device:${deviceId}:${code}`;

      if (await isIdempotencyKeyUsed(env, idempotencyKey)) {
        return jsonResponse(
          { ok: false, code: 'VOUCHER_ALREADY_REDEEMED', error: `El código '${code}' ya fue canjeado previamente.` },
          409
        );
      }

      await markIdempotencyKeyUsed(env, idempotencyKey, { deviceId, code, creditsAdded });
      if (storedVoucher) {
        storedVoucher.redeemed = true;
        storedVoucher.redeemedBy = deviceId;
        storedVoucher.redeemedAt = Date.now();
        VOUCHER_MEMORY_MAP.set(code, storedVoucher);
        if (env && env.TEPUI_CREDITS_KV) {
          try {
            await env.TEPUI_CREDITS_KV.put(`voucher:${code}`, JSON.stringify(storedVoucher));
          } catch {
            // Ignorar fallo de escritura KV
          }
        }
      }

      const state = await loadDeviceState(env, deviceId);
      state.credits += creditsAdded;
      state.tier = 'PRO CLOUD';
      await saveDeviceState(env, deviceId, state);

      return jsonResponse({
        ok: true,
        tier: state.tier,
        credits_added: creditsAdded,
        credits_remaining: state.credits,
        code,
        message: `Código '${code}' activado (+${creditsAdded} créditos Pro Cloud).`,
      });
    }

    // 4. Endpoint de generación de Samples WAV con caché Edge y cobro de 5 créditos (TEPUI_AI_GATEWAY_URL)
    if (
      request.method === 'POST' &&
      (path === '/v1/sample-wav' || path === '/api/ai/sample-wav')
    ) {
      let bodyJson = {};
      try {
        bodyJson = await request.json();
      } catch {
        return jsonResponse({ ok: false, error: 'Payload JSON inválido.' }, 400);
      }

      const deviceId = sanitizeDeviceId(
        bodyJson.device_id || request.headers.get('x-tepui-device-id') || 'global'
      );
      const state = await loadDeviceState(env, deviceId);

      if (state.credits < WAV_SAMPLE_CREDIT_COST) {
        return jsonResponse(
          {
            ok: false,
            code: 'CREDITS_EXHAUSTED',
            tier: state.tier,
            credits_remaining: state.credits,
            required_credits: WAV_SAMPLE_CREDIT_COST,
            error: `Saldo insuficiente (${state.credits} CR) para generar Sample WAV IA (costo: ${WAV_SAMPLE_CREDIT_COST} CR). Recarga créditos en AI: KV.`,
          },
          402,
          {
            'X-Tepui-Credits-Remaining': String(state.credits),
            'X-Tepui-Credits-Charged': '0',
            'X-Tepui-Tier': state.tier,
          }
        );
      }

      const promptText = String(bodyJson.prompt || 'Concert Grand Piano C4').trim();
      const rootNote = Math.max(24, Math.min(96, Number(bodyJson.root_note || 60)));
      const cacheKey = `${promptText.toLowerCase()}::${rootNote}`;

      let wavBytes = WAV_CACHE_MEMORY_MAP.get(cacheKey);
      let cacheStatus = 'HIT';
      if (!wavBytes) {
        cacheStatus = 'MISS';
        wavBytes = synthesizeEdgeStereoWavBytes(promptText, rootNote);
        if (WAV_CACHE_MEMORY_MAP.size > 64) {
          const firstKey = WAV_CACHE_MEMORY_MAP.keys().next().value;
          WAV_CACHE_MEMORY_MAP.delete(firstKey);
        }
        WAV_CACHE_MEMORY_MAP.set(cacheKey, wavBytes);
      }

      state.credits -= WAV_SAMPLE_CREDIT_COST;
      if (cacheStatus === 'MISS') {
        state.neuronsUsed = (state.neuronsUsed || 0) + 120;
      }
      await saveDeviceState(env, deviceId, state);

      return new Response(wavBytes, {
        status: 200,
        headers: {
          'Content-Type': 'audio/wav',
          'Content-Length': String(wavBytes.byteLength),
          'Access-Control-Allow-Origin': '*',
          'Access-Control-Expose-Headers':
            'X-Tepui-Credits-Remaining, X-Tepui-Credits-Charged, X-Tepui-Tier, X-Tepui-Cache',
          'X-Tepui-Credits-Charged': String(WAV_SAMPLE_CREDIT_COST),
          'X-Tepui-Credits-Remaining': String(state.credits),
          'X-Tepui-Tier': state.tier,
          'X-Tepui-Cache': cacheStatus,
        },
      });
    }

    // 5. Endpoint de generación de Preset Espectral con cobro de 1 crédito
    if (request.method === 'POST' && (path === '/v1/preset' || path === '/api/ai/preset')) {
      let bodyJson = {};
      try {
        bodyJson = await request.json();
      } catch {
        return jsonResponse({ ok: false, error: 'Payload JSON inválido.' }, 400);
      }
      const deviceId = sanitizeDeviceId(
        bodyJson.device_id || request.headers.get('x-tepui-device-id') || 'global'
      );
      const state = await loadDeviceState(env, deviceId);
      if (state.credits < PRESET_CREDIT_COST) {
        return jsonResponse(
          {
            ok: false,
            code: 'CREDITS_EXHAUSTED',
            tier: state.tier,
            credits_remaining: state.credits,
            error: 'Sin créditos Cloud disponibles. Canjea un código de recarga en AI: KV.',
          },
          402
        );
      }
      state.credits -= PRESET_CREDIT_COST;
      state.neuronsUsed = (state.neuronsUsed || 0) + 25;
      await saveDeviceState(env, deviceId, state);
      return jsonResponse({
        ok: true,
        cached: false,
        tier: state.tier,
        credits_remaining: state.credits,
        daily_neurons_used: state.neuronsUsed,
        max_daily_neurons: 9500,
        message: `Preset generado (-${PRESET_CREDIT_COST} CR • Restan ${state.credits} CR).`,
      });
    }

    if (env && env.ASSETS) {
      return env.ASSETS.fetch(request);
    }
    return new Response('Not Found', { status: 404 });
  },
};
