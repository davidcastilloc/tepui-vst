/**
 * Cloudflare Worker Edge para TEPUI VST (`tepui-vst`)
 * Sirve activos estáticos desde `site/` (`env.ASSETS`) y expone:
 * - POST `/api/webhooks/checkout` y `/v1/webhooks/checkout` (verificación HMAC-SHA256 e idempotencia)
 * - POST `/api/credits/redeem` y `/v1/credits/redeem` (canje idempotente de códigos Cloud)
 */

const REDEEMED_MEMORY_SET = new Set();

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

function jsonResponse(obj, status = 200) {
  return new Response(JSON.stringify(obj), {
    status,
    headers: {
      'Content-Type': 'application/json; charset=utf-8',
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
      'Access-Control-Allow-Headers': 'Content-Type, X-Signature-256, X-Hook-Signature, X-Tepui-Device-Id, X-Tepui-License-Key',
    },
  });
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

      if (REDEEMED_MEMORY_SET.has(eventId)) {
        return jsonResponse({
          ok: true,
          cached: true,
          tier: 'PRO CLOUD',
          credits_added: 0,
          message: `Evento ${eventId} ya procesado previamente (idempotente).`,
        });
      }

      const credits = Math.max(1, Math.min(100000, Number(bodyJson.credits || 500)));
      REDEEMED_MEMORY_SET.add(eventId);
      return jsonResponse({
        ok: true,
        cached: false,
        tier: 'PRO CLOUD',
        credits_added: credits,
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
      return jsonResponse({
        ok: true,
        tier: 'PRO CLOUD',
        credits_added: creditsAdded,
        code,
        message: `Código '${code}' activado (+${creditsAdded} créditos Pro Cloud).`,
      });
    }

    if (env && env.ASSETS) {
      return env.ASSETS.fetch(request);
    }
    return new Response('Not Found', { status: 404 });
  },
};
