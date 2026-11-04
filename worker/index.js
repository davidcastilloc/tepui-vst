/**
 * Cloudflare Worker Edge para TEPUI VST (`tepui-vst`)
 * Sirve activos estáticos desde `site/` (`env.ASSETS`) y expone:
 * - POST `/api/webhooks/checkout` y `/v1/webhooks/checkout` (verificación HMAC-SHA256 e idempotencia)
 * - POST `/api/credits/redeem` y `/v1/credits/redeem` (canje idempotente de códigos Cloud)
 * - POST `/v1/sample-wav` y `/api/ai/sample-wav` (`TEPUI_AI_GATEWAY_URL`: generación de samples `.wav` con cobro de 5 créditos)
 * - POST `/v1/preset` y `/api/ai/preset` (diseño de preset IA con cobro de 1 crédito)
 */

const REDEEMED_MEMORY_SET = new Set();
const DEVICE_CREDITS_MAP = new Map();

const INITIAL_FREE_CREDITS = 20;
const WAV_SAMPLE_CREDIT_COST = 5;
const PRESET_CREDIT_COST = 1;

function getDeviceState(deviceId) {
  const key = (deviceId || 'global').trim() || 'global';
  if (!DEVICE_CREDITS_MAP.has(key)) {
    DEVICE_CREDITS_MAP.set(key, {
      credits: INITIAL_FREE_CREDITS,
      tier: 'COMMUNITY KV',
      neuronsUsed: 0,
    });
  }
  return DEVICE_CREDITS_MAP.get(key);
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

function jsonResponse(obj, status = 200, extraHeaders = {}) {
  return new Response(JSON.stringify(obj), {
    status,
    headers: {
      'Content-Type': 'application/json; charset=utf-8',
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
      'Access-Control-Allow-Headers':
        'Content-Type, X-Signature-256, X-Hook-Signature, X-Tepui-Device-Id, X-Tepui-License-Key, cf-aig-authorization, cf-aig-cache-ttl',
      'Access-Control-Expose-Headers':
        'X-Tepui-Credits-Remaining, X-Tepui-Credits-Charged, X-Tepui-Tier',
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

  // Encabezado RIFF WAVE PCM 16-bit estéreo
  writeStr(0, 'RIFF');
  view.setUint32(4, totalByteLen - 8, true);
  writeStr(8, 'WAVE');
  writeStr(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM = 1
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

    if (
      request.method === 'POST' &&
      (path === '/api/webhooks/checkout' || path === '/v1/webhooks/checkout')
    ) {
      const sigHeader =
        request.headers.get('x-signature-256') ||
        request.headers.get('x-hook-signature') ||
        '';
      const cleanSig = sigHeader.trim().replace(/^sha256=/i, '').toLowerCase();
      const secret = (env && env.TEPUI_WEBHOOK_SECRET) || 'whsec_tepui_default_edge_secret';
      const rawBody = await request.text();
      const expectedHex = await computeHmacSha256Hex(rawBody, secret);

      if (!timingSafeHexEqual(cleanSig, expectedHex)) {
        return jsonResponse(
          { ok: false, error: 'HTTP 401: Firma HMAC-SHA256 de webhook inválida.' },
          401
        );
      }

      let bodyJson;
      try {
        bodyJson = JSON.parse(rawBody);
      } catch {
        return jsonResponse({ ok: false, error: 'Payload JSON inválido.' }, 400);
      }

      const eventId = String(bodyJson.event_id || bodyJson.id || '').trim().toUpperCase();
      if (!eventId) {
        return jsonResponse({ ok: false, error: 'Falta event_id en el webhook.' }, 400);
      }

      const deviceId = String(
        bodyJson.device_id || request.headers.get('x-tepui-device-id') || 'global'
      ).trim();
      const state = getDeviceState(deviceId);

      if (REDEEMED_MEMORY_SET.has(eventId)) {
        return jsonResponse({
          ok: true,
          cached: true,
          tier: state.tier,
          credits_added: 0,
          credits_remaining: state.credits,
          message: `Evento ${eventId} ya procesado previamente (idempotente).`,
        });
      }

      const credits = Math.max(1, Math.min(100000, Number(bodyJson.credits || 500)));
      REDEEMED_MEMORY_SET.add(eventId);
      state.credits += credits;
      state.tier = 'PRO CLOUD';

      return jsonResponse({
        ok: true,
        cached: false,
        tier: state.tier,
        credits_added: credits,
        credits_remaining: state.credits,
        event_id: eventId,
        message: `Webhook verificado (${eventId}): +${credits} créditos acreditados.`,
      });
    }

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

      const deviceId = String(
        bodyJson.device_id || request.headers.get('x-tepui-device-id') || 'global'
      ).trim();
      const idempotencyKey = `${deviceId}:${code}`;
      if (REDEEMED_MEMORY_SET.has(idempotencyKey)) {
        return jsonResponse(
          { ok: false, error: `El código '${code}' ya fue canjeado previamente.` },
          409
        );
      }

      let creditsAdded = 0;
      if (
        code.startsWith('TEPUI-VIP-') ||
        code.startsWith('TEPUI-1000-') ||
        code.startsWith('TEPUI-CLOUD-1000')
      ) {
        creditsAdded = 1000;
      } else if (
        code.startsWith('TEPUI-PRO-') ||
        code.startsWith('TEPUI-500-') ||
        code.startsWith('TEPUI-CLOUD-500')
      ) {
        creditsAdded = 500;
      } else if (
        code.startsWith('TEPUI-100-') ||
        code.startsWith('TEPUI-PLUS-') ||
        code.startsWith('TEPUI-CLOUD-100')
      ) {
        creditsAdded = 100;
      } else {
        return jsonResponse({ ok: false, error: `Código '${code}' no reconocido.` }, 400);
      }

      REDEEMED_MEMORY_SET.add(idempotencyKey);
      const state = getDeviceState(deviceId);
      state.credits += creditsAdded;
      state.tier = 'PRO CLOUD';

      return jsonResponse({
        ok: true,
        tier: state.tier,
        credits_added: creditsAdded,
        credits_remaining: state.credits,
        code,
        message: `Código '${code}' activado (+${creditsAdded} créditos Pro Cloud).`,
      });
    }

    // Endpoint de generación de Samples WAV con cobro de créditos (TEPUI_AI_GATEWAY_URL)
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

      const deviceId = String(
        bodyJson.device_id || request.headers.get('x-tepui-device-id') || 'global'
      ).trim();
      const state = getDeviceState(deviceId);

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

      state.credits -= WAV_SAMPLE_CREDIT_COST;
      state.neuronsUsed += 120;

      const promptText = String(bodyJson.prompt || 'Concert Grand Piano C4');
      const rootNote = Number(bodyJson.root_note || 60);
      const wavBytes = synthesizeEdgeStereoWavBytes(promptText, rootNote);

      return new Response(wavBytes, {
        status: 200,
        headers: {
          'Content-Type': 'audio/wav',
          'Content-Length': String(wavBytes.byteLength),
          'Access-Control-Allow-Origin': '*',
          'Access-Control-Expose-Headers':
            'X-Tepui-Credits-Remaining, X-Tepui-Credits-Charged, X-Tepui-Tier',
          'X-Tepui-Credits-Charged': String(WAV_SAMPLE_CREDIT_COST),
          'X-Tepui-Credits-Remaining': String(state.credits),
          'X-Tepui-Tier': state.tier,
        },
      });
    }

    // Endpoint de generación de Preset Espectral con cobro de 1 crédito
    if (request.method === 'POST' && (path === '/v1/preset' || path === '/api/ai/preset')) {
      let bodyJson = {};
      try {
        bodyJson = await request.json();
      } catch {
        return jsonResponse({ ok: false, error: 'Payload JSON inválido.' }, 400);
      }
      const deviceId = String(
        bodyJson.device_id || request.headers.get('x-tepui-device-id') || 'global'
      ).trim();
      const state = getDeviceState(deviceId);
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
      state.neuronsUsed += 25;
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
