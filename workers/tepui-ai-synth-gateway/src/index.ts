export interface AiBinding {
  run(
    model: string,
    inputs: Record<string, unknown>,
    options?: Record<string, unknown>
  ): Promise<unknown>;
}

export interface KvNamespaceBinding {
  get(key: string, type?: "text"): Promise<string | null>;
  get<T = unknown>(key: string, type: "json"): Promise<T | null>;
  put(
    key: string,
    value: string,
    options?: { expirationTtl?: number }
  ): Promise<void>;
}

export interface Env {
  AI: AiBinding;
  TEPUI_KV: KvNamespaceBinding;
  AI_GATEWAY_URL?: string;
  AI_GATEWAY_TOKEN?: string;
}

export interface ModalPartialSpec {
  ratio: number;
  amplitude: number;
  decay_rate: number;
  detune_cents: number;
  pan: number;
}

export interface AiSynthesizerPatchSpec {
  name: string;
  category: string;
  root_note: number;
  attack: number;
  decay: number;
  sustain: number;
  release: number;
  cutoff: number;
  resonance: number;
  filter_mode: number;
  lfo_rate: number;
  lfo_depth: number;
  lfo_target: number;
  arp_mode: number;
  space_mix: number;
  mod_matrix_mask: number;
  brightness: number;
  exciter_noise: number;
  body_warmth: number;
  fm_index: number;
  fm_ratio: number;
  loop_enabled: boolean;
  partials: ModalPartialSpec[];
}

interface LicenseRecord {
  key: string;
  plan: string;
  credits_remaining: number;
  total_used: number;
  updated_at: string;
}

const MODEL_ID = "@cf/meta/llama-3.3-70b-instruct-fp8-fast";

// Escudo Anti-Factura: 9,500 Neurons/día máximo (por debajo de los 10,000 Neurons/día gratuitos de Cloudflare = $0.00 riesgo)
const MAX_DAILY_FREE_POOL_NEURONS = 9500;
const ESTIMATED_NEURONS_PER_GEN = 85;
const FREE_DAILY_CREDITS_PER_DEVICE = 15;

// Claves Pro pre-autorizadas para activación inmediata en KV
const BUILTIN_PRO_LICENSES: Record<string, { plan: string; initialCredits: number }> = {
  "TEPUI-PRO-FOUNDER-2026": { plan: "PRO FOUNDER (1,000 CR)", initialCredits: 1000 },
  "TEPUI-PRO-PACK-500": { plan: "CLOUD PACK (500 CR)", initialCredits: 500 },
  "TEPUI-STUDIO-VIP": { plan: "STUDIO VIP (2,500 CR)", initialCredits: 2500 }
};

function clamp(v: number, lo: number, hi: number): number {
  if (!Number.isFinite(v)) return lo;
  return Math.max(lo, Math.min(hi, v));
}

function utcDateKey(): string {
  return new Date().toISOString().slice(0, 10);
}

async function sha256Hex(input: string): Promise<string> {
  const bytes = new TextEncoder().encode(input);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function sanitizePatchSpec(
  raw: Partial<AiSynthesizerPatchSpec>,
  prompt: string
): AiSynthesizerPatchSpec {
  const rawPartials = Array.isArray(raw.partials) ? raw.partials : [];
  const partials: ModalPartialSpec[] = [];
  for (let i = 0; i < Math.min(10, rawPartials.length); i++) {
    const p = rawPartials[i];
    partials.push({
      ratio: clamp(Number(p?.ratio) || i + 1, 0.25, 24.0),
      amplitude: clamp(Number(p?.amplitude) || 1.0 / (i + 1), 0.001, 1.0),
      decay_rate: clamp(Number(p?.decay_rate) || 1.2 + i * 0.45, 0.15, 25.0),
      detune_cents: clamp(Number(p?.detune_cents) || 0.0, -50.0, 50.0),
      pan: clamp(Number(p?.pan) || 0.0, -1.0, 1.0)
    });
  }
  while (partials.length < 8) {
    const k = partials.length + 1;
    partials.push({
      ratio: k,
      amplitude: clamp(Math.pow(k, -1.25), 0.01, 1.0),
      decay_rate: 1.1 + k * 0.45,
      detune_cents: (k % 2 === 0 ? 1 : -1) * (k * 0.6),
      pan: ((k % 3) - 1) * 0.18
    });
  }

  return {
    name: String(raw.name || prompt || "TEPUI AI Instrument").slice(0, 48),
    category: String(raw.category || "Hybrid Synth").slice(0, 24),
    root_note: Math.round(clamp(Number(raw.root_note ?? 60), 24, 96)),
    attack: clamp(Number(raw.attack ?? 0.015), 0.001, 5.0),
    decay: clamp(Number(raw.decay ?? 0.35), 0.01, 5.0),
    sustain: clamp(Number(raw.sustain ?? 0.7), 0.0, 1.0),
    release: clamp(Number(raw.release ?? 0.4), 0.01, 10.0),
    cutoff: clamp(Number(raw.cutoff ?? 9500.0), 60.0, 20000.0),
    resonance: clamp(Number(raw.resonance ?? 1.15), 0.1, 8.5),
    filter_mode: Math.round(clamp(Number(raw.filter_mode ?? 0), 0, 3)),
    lfo_rate: clamp(Number(raw.lfo_rate ?? 3.8), 0.1, 20.0),
    lfo_depth: clamp(Number(raw.lfo_depth ?? 0.18), 0.0, 1.0),
    lfo_target: Math.round(clamp(Number(raw.lfo_target ?? 0), 0, 3)),
    arp_mode: Math.round(clamp(Number(raw.arp_mode ?? 0), 0, 4)),
    space_mix: clamp(Number(raw.space_mix ?? 0.24), 0.0, 1.0),
    mod_matrix_mask: Math.round(clamp(Number(raw.mod_matrix_mask ?? 2178), 0, 0x01ffffff)),
    brightness: clamp(Number(raw.brightness ?? 0.65), 0.0, 1.0),
    exciter_noise: clamp(Number(raw.exciter_noise ?? 0.22), 0.0, 1.0),
    body_warmth: clamp(Number(raw.body_warmth ?? 0.55), 0.0, 1.0),
    fm_index: clamp(Number(raw.fm_index ?? 0.6), 0.0, 6.0),
    fm_ratio: clamp(Number(raw.fm_ratio ?? 2.0), 0.5, 12.0),
    loop_enabled: Boolean(raw.loop_enabled ?? true),
    partials
  };
}

const SYSTEM_PROMPT =
  "You are TEPUI Gran Sabana Sonic Lab's synthesizer patch & modal-spectral sample architect. " +
  "Given a user musical description, return ONLY a compact raw JSON object (no markdown fences) for a playable polyphonic Piano Roll instrument (NEVER speech or TTS) with these exact keys: " +
  '{"name":string,"category":string,"root_note":int(24..96),"attack":float(0.001..2.0),"decay":float(0.05..3.0),"sustain":float(0..1),"release":float(0.05..4.0),"cutoff":float(120..20000),"resonance":float(0.5..6.0),"filter_mode":int(0=LP,1=HP,2=BP),"lfo_rate":float(0.2..16),"lfo_depth":float(0..0.9),"lfo_target":int(0=Cutoff,1=Pitch,2=Amp),"arp_mode":int(0=Off,1=Up,2=Down,3=Joropo 6/8,4=Merengue 5/8),"space_mix":float(0..0.8),"mod_matrix_mask":int(0..33554431),"brightness":float(0..1),"exciter_noise":float(0..1),"body_warmth":float(0..1),"fm_index":float(0..4),"fm_ratio":float(0.5..8),"loop_enabled":bool,"partials":[{"ratio":float,"amplitude":float,"decay_rate":float,"detune_cents":float,"pan":float}]} (8 partials).';

async function generatePatchSpecWithWorkersAi(
  env: Env,
  prompt: string,
  byokAccount?: string,
  byokToken?: string
): Promise<AiSynthesizerPatchSpec> {
  if (byokAccount && byokToken) {
    const url = `https://api.cloudflare.com/client/v4/accounts/${byokAccount.trim()}/ai/run/${MODEL_ID}`;
    const resp = await fetch(url, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${byokToken.trim()}`,
        "Content-Type": "application/json"
      },
      body: JSON.stringify({
        messages: [
          { role: "system", content: SYSTEM_PROMPT },
          { role: "user", content: prompt }
        ],
        temperature: 0.2,
        max_tokens: 550,
        response_format: { type: "json_object" }
      })
    });
    if (!resp.ok) {
      throw new Error(`BYOK Cloudflare API error: HTTP ${resp.status}`);
    }
    const data = (await resp.json()) as Record<string, unknown>;
    const resObj = (data.result ?? data) as Record<string, unknown>;
    const rawField = resObj.response ?? resObj;
    const parsed =
      typeof rawField === "string"
        ? (JSON.parse(rawField) as Partial<AiSynthesizerPatchSpec>)
        : (rawField as Partial<AiSynthesizerPatchSpec>);
    return sanitizePatchSpec(parsed, prompt);
  }

  const rawResult = await env.AI.run(MODEL_ID, {
    messages: [
      { role: "system", content: SYSTEM_PROMPT },
      { role: "user", content: prompt }
    ],
    temperature: 0.2,
    max_tokens: 550,
    response_format: {
      type: "json_object"
    }
  });

  let parsed: Partial<AiSynthesizerPatchSpec> = {};
  if (rawResult && typeof rawResult === "object") {
    const obj = rawResult as Record<string, unknown>;
    const respField = obj.response ?? obj.result ?? obj;
    if (typeof respField === "string") {
      parsed = JSON.parse(respField) as Partial<AiSynthesizerPatchSpec>;
    } else if (respField && typeof respField === "object") {
      parsed = respField as Partial<AiSynthesizerPatchSpec>;
    }
  }
  return sanitizePatchSpec(parsed, prompt);
}

async function getDailyNeuronsUsed(env: Env): Promise<number> {
  const key = `budget:neurons:${utcDateKey()}`;
  const raw = await env.TEPUI_KV.get(key, "text");
  return raw ? parseInt(raw, 10) || 0 : 0;
}

async function addDailyNeuronsUsed(env: Env, delta: number): Promise<number> {
  const key = `budget:neurons:${utcDateKey()}`;
  const current = await getDailyNeuronsUsed(env);
  const next = current + delta;
  await env.TEPUI_KV.put(key, String(next), { expirationTtl: 60 * 60 * 48 });
  return next;
}

async function resolveLicenseOrDeviceQuota(
  env: Env,
  licenseKey: string,
  deviceId: string,
  byokAccount: string,
  byokToken: string
): Promise<{
  allowed: boolean;
  tier: string;
  creditsRemaining: number;
  errorCode?: string;
  errorMessage?: string;
  licenseRecord?: LicenseRecord;
}> {
  if (byokAccount && byokToken) {
    return {
      allowed: true,
      tier: "BYOK (USER API)",
      creditsRemaining: 999999
    };
  }

  const cleanLicense = licenseKey.trim().toUpperCase();
  if (cleanLicense.length >= 8) {
    const kvKey = `license:${cleanLicense}`;
    let record = await env.TEPUI_KV.get<LicenseRecord>(kvKey, "json");
    if (!record && BUILTIN_PRO_LICENSES[cleanLicense]) {
      const preset = BUILTIN_PRO_LICENSES[cleanLicense];
      record = {
        key: cleanLicense,
        plan: preset.plan,
        credits_remaining: preset.initialCredits,
        total_used: 0,
        updated_at: new Date().toISOString()
      };
      await env.TEPUI_KV.put(kvKey, JSON.stringify(record));
    }
    if (!record) {
      return {
        allowed: false,
        tier: "INVALID LICENSE",
        creditsRemaining: 0,
        errorCode: "INVALID_LICENSE_KEY",
        errorMessage: "La clave de licencia ingresada no existe o expiró."
      };
    }
    if (record.credits_remaining <= 0) {
      return {
        allowed: false,
        tier: record.plan,
        creditsRemaining: 0,
        errorCode: "CREDITS_EXHAUSTED",
        errorMessage: "Tus créditos Cloud IA se agotaron. Recarga tu licencia o usa BYOK / NPU Local."
      };
    }
    return {
      allowed: true,
      tier: record.plan,
      creditsRemaining: record.credits_remaining,
      licenseRecord: record
    };
  }

  // Free Tier por Dispositivo (15 generaciones Cloud/día)
  const dateStr = utcDateKey();
  const quotaKey = `quota:${dateStr}:${deviceId}`;
  const usedRaw = await env.TEPUI_KV.get(quotaKey, "text");
  const usedToday = usedRaw ? parseInt(usedRaw, 10) || 0 : 0;
  const remaining = Math.max(0, FREE_DAILY_CREDITS_PER_DEVICE - usedToday);
  if (remaining <= 0) {
    return {
      allowed: false,
      tier: "FREE TIER (0/15)",
      creditsRemaining: 0,
      errorCode: "FREE_QUOTA_EXCEEDED",
      errorMessage:
        "Alcanzaste las 15 generaciones Cloud IA gratuitas de hoy. Activa una Licencia Pro, usa BYOK o continúa con el motor NPU Local ilimitado."
    };
  }
  return {
    allowed: true,
    tier: "FREE TIER",
    creditsRemaining: remaining
  };
}

async function consumeOneCredit(
  env: Env,
  deviceId: string,
  licenseRecord?: LicenseRecord
): Promise<number> {
  if (licenseRecord) {
    licenseRecord.credits_remaining = Math.max(0, licenseRecord.credits_remaining - 1);
    licenseRecord.total_used = (licenseRecord.total_used || 0) + 1;
    licenseRecord.updated_at = new Date().toISOString();
    await env.TEPUI_KV.put(
      `license:${licenseRecord.key}`,
      JSON.stringify(licenseRecord)
    );
    return licenseRecord.credits_remaining;
  }

  const dateStr = utcDateKey();
  const quotaKey = `quota:${dateStr}:${deviceId}`;
  const usedRaw = await env.TEPUI_KV.get(quotaKey, "text");
  const usedToday = (usedRaw ? parseInt(usedRaw, 10) || 0 : 0) + 1;
  await env.TEPUI_KV.put(quotaKey, String(usedToday), { expirationTtl: 60 * 60 * 36 });
  return Math.max(0, FREE_DAILY_CREDITS_PER_DEVICE - usedToday);
}

const CORS_HEADERS: Record<string, string> = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
  "Access-Control-Allow-Headers":
    "Content-Type, Authorization, X-Tepui-License-Key, X-Tepui-Device-Id, X-Tepui-Byok-Account, X-Tepui-Byok-Token"
};

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }

    const url = new URL(request.url);
    const startMs = Date.now();
    const clientIp = request.headers.get("CF-Connecting-IP") || "anon-ip";
    const deviceId = (request.headers.get("X-Tepui-Device-Id") || clientIp)
      .trim()
      .slice(0, 64);
    const licenseKey = (request.headers.get("X-Tepui-License-Key") || "").trim();
    const byokAccount = (request.headers.get("X-Tepui-Byok-Account") || "").trim();
    const byokToken = (request.headers.get("X-Tepui-Byok-Token") || "").trim();

    try {
      if (url.pathname === "/health" || url.pathname === "/") {
        const dailyNeurons = await getDailyNeuronsUsed(env);
        return new Response(
          JSON.stringify({
            ok: true,
            service: "tepui-ai-synth-gateway",
            model: MODEL_ID,
            engine: "CLOUDFLARE-WORKERS-AI",
            kv_cache: true,
            daily_neurons_used: dailyNeurons,
            max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS
          }),
          {
            status: 200,
            headers: { ...CORS_HEADERS, "Content-Type": "application/json; charset=utf-8" }
          }
        );
      }

      if (url.pathname === "/v1/quota" || url.pathname === "/v1/license/verify") {
        const quota = await resolveLicenseOrDeviceQuota(
          env,
          licenseKey,
          deviceId,
          byokAccount,
          byokToken
        );
        const dailyNeurons = await getDailyNeuronsUsed(env);
        return new Response(
          JSON.stringify({
            ok: quota.allowed,
            tier: quota.tier,
            credits_remaining: quota.creditsRemaining,
            daily_neurons_used: dailyNeurons,
            max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
            error_code: quota.errorCode ?? null,
            message: quota.errorMessage ?? "Cuenta activa y protegida con Escudo Anti-Costos KV."
          }),
          {
            status: 200,
            headers: { ...CORS_HEADERS, "Content-Type": "application/json; charset=utf-8" }
          }
        );
      }

      if (url.pathname === "/v1/preset" && request.method === "POST") {
        const body = (await request.json().catch(() => ({}))) as { prompt?: string };
        const prompt = String(body.prompt || "Concert Grand Piano C4").trim().slice(0, 280);
        const normalizedPrompt = prompt.toLowerCase().replace(/\s+/g, " ");
        const promptHash = await sha256Hex(normalizedPrompt);
        const cacheKey = `cache:preset:v1:${promptHash}`;

        const quota = await resolveLicenseOrDeviceQuota(
          env,
          licenseKey,
          deviceId,
          byokAccount,
          byokToken
        );

        // 1. Verificar Caché Global en Cloudflare KV (0 Neurons = $0.00 costo y <10ms latencia)
        const cachedSpec = await env.TEPUI_KV.get<AiSynthesizerPatchSpec>(cacheKey, "json");
        if (cachedSpec) {
          const dailyNeurons = await getDailyNeuronsUsed(env);
          const elapsedMs = Date.now() - startMs;
          console.log(
            JSON.stringify({
              event: "preset_kv_cache_hit",
              prompt,
              tier: quota.tier,
              neurons_used: 0,
              elapsed_ms: elapsedMs
            })
          );
          return new Response(
            JSON.stringify({
              ok: true,
              backend: "CLOUDFLARE-KV-EDGE-CACHE",
              cached: true,
              neurons_used: 0,
              tier: quota.tier,
              credits_remaining: quota.creditsRemaining,
              daily_neurons_used: dailyNeurons,
              max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
              model: MODEL_ID,
              elapsed_ms: elapsedMs,
              spec: cachedSpec
            }),
            {
              status: 200,
              headers: { ...CORS_HEADERS, "Content-Type": "application/json; charset=utf-8" }
            }
          );
        }

        // 2. Si no está en caché, validar cuota de usuario / licencia
        if (!quota.allowed) {
          const dailyNeurons = await getDailyNeuronsUsed(env);
          return new Response(
            JSON.stringify({
              ok: false,
              code: quota.errorCode || "QUOTA_EXCEEDED",
              error: quota.errorMessage || "Sin créditos Cloud IA disponibles.",
              fallback: "LOCAL_NPU",
              tier: quota.tier,
              credits_remaining: 0,
              daily_neurons_used: dailyNeurons,
              max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS
            }),
            {
              status: 402,
              headers: { ...CORS_HEADERS, "Content-Type": "application/json; charset=utf-8" }
            }
          );
        }

        // 3. Validar Cortafuegos Financiero Diario (9,500 Neurons/día máximo si no es BYOK)
        const isByok = Boolean(byokAccount && byokToken);
        const currentDailyNeurons = await getDailyNeuronsUsed(env);
        if (
          !isByok &&
          currentDailyNeurons + ESTIMATED_NEURONS_PER_GEN > MAX_DAILY_FREE_POOL_NEURONS
        ) {
          return new Response(
            JSON.stringify({
              ok: false,
              code: "DAILY_BUDGET_GUARD",
              error:
                "Tope diario gratuito de seguridad alcanzado (9,500 Neurons = $0 costo). Conmutando automáticamente al motor NPU Local.",
              fallback: "LOCAL_NPU",
              tier: quota.tier,
              credits_remaining: quota.creditsRemaining,
              daily_neurons_used: currentDailyNeurons,
              max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS
            }),
            {
              status: 429,
              headers: { ...CORS_HEADERS, "Content-Type": "application/json; charset=utf-8" }
            }
          );
        }

        // 4. Ejecutar inferencia en Cloudflare Workers AI (o cuenta BYOK del usuario)
        const spec = await generatePatchSpecWithWorkersAi(
          env,
          prompt,
          byokAccount || undefined,
          byokToken || undefined
        );

        // 5. Guardar en Caché Global Cloudflare KV por 30 días para que futuras peticiones cuesten 0 Neurons
        await env.TEPUI_KV.put(cacheKey, JSON.stringify(spec), {
          expirationTtl: 60 * 60 * 24 * 30
        });

        // 6. Contabilizar Neurons y descontar 1 crédito (si no es BYOK)
        const updatedDailyNeurons = isByok
          ? currentDailyNeurons
          : await addDailyNeuronsUsed(env, ESTIMATED_NEURONS_PER_GEN);
        const updatedCredits = isByok
          ? 999999
          : await consumeOneCredit(env, deviceId, quota.licenseRecord);

        const elapsedMs = Date.now() - startMs;
        console.log(
          JSON.stringify({
            event: "preset_synthesized_gpu",
            prompt,
            tier: quota.tier,
            credits_remaining: updatedCredits,
            daily_neurons_used: updatedDailyNeurons,
            elapsed_ms: elapsedMs
          })
        );

        return new Response(
          JSON.stringify({
            ok: true,
            backend: isByok ? "CLOUDFLARE-BYOK-AI" : "CLOUDFLARE-WORKERS-AI",
            cached: false,
            neurons_used: isByok ? 0 : ESTIMATED_NEURONS_PER_GEN,
            tier: quota.tier,
            credits_remaining: updatedCredits,
            daily_neurons_used: updatedDailyNeurons,
            max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
            model: MODEL_ID,
            elapsed_ms: elapsedMs,
            spec
          }),
          {
            status: 200,
            headers: { ...CORS_HEADERS, "Content-Type": "application/json; charset=utf-8" }
          }
        );
      }

      return new Response(JSON.stringify({ ok: false, error: "Not Found" }), {
        status: 404,
        headers: { ...CORS_HEADERS, "Content-Type": "application/json; charset=utf-8" }
      });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(JSON.stringify({ event: "worker_error", path: url.pathname, error: msg }));
      return new Response(JSON.stringify({ ok: false, error: msg }), {
        status: 500,
        headers: { ...CORS_HEADERS, "Content-Type": "application/json; charset=utf-8" }
      });
    }
  }
};
