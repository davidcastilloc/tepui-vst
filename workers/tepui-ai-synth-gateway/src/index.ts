export interface AiBinding {
  run(
    model: string,
    inputs: Record<string, unknown>,
    options?: Record<string, unknown>
  ): Promise<unknown>;
}

export interface Env {
  AI: AiBinding;
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

const MODEL_ID = "@cf/meta/llama-3.3-70b-instruct-fp8-fast";

const PATCH_JSON_SCHEMA = {
  type: "object",
  required: [
    "name",
    "category",
    "root_note",
    "attack",
    "decay",
    "sustain",
    "release",
    "cutoff",
    "resonance",
    "filter_mode",
    "lfo_rate",
    "lfo_depth",
    "lfo_target",
    "arp_mode",
    "space_mix",
    "mod_matrix_mask",
    "brightness",
    "exciter_noise",
    "body_warmth",
    "fm_index",
    "fm_ratio",
    "loop_enabled",
    "partials"
  ],
  properties: {
    name: { type: "string" },
    category: { type: "string" },
    root_note: { type: "integer", minimum: 24, maximum: 96 },
    attack: { type: "number", minimum: 0.001, maximum: 5.0 },
    decay: { type: "number", minimum: 0.01, maximum: 5.0 },
    sustain: { type: "number", minimum: 0.0, maximum: 1.0 },
    release: { type: "number", minimum: 0.01, maximum: 10.0 },
    cutoff: { type: "number", minimum: 60.0, maximum: 20000.0 },
    resonance: { type: "number", minimum: 0.1, maximum: 8.5 },
    filter_mode: { type: "integer", minimum: 0, maximum: 3 },
    lfo_rate: { type: "number", minimum: 0.1, maximum: 20.0 },
    lfo_depth: { type: "number", minimum: 0.0, maximum: 1.0 },
    lfo_target: { type: "integer", minimum: 0, maximum: 3 },
    arp_mode: { type: "integer", minimum: 0, maximum: 4 },
    space_mix: { type: "number", minimum: 0.0, maximum: 1.0 },
    mod_matrix_mask: { type: "integer", minimum: 0, maximum: 33554431 },
    brightness: { type: "number", minimum: 0.0, maximum: 1.0 },
    exciter_noise: { type: "number", minimum: 0.0, maximum: 1.0 },
    body_warmth: { type: "number", minimum: 0.0, maximum: 1.0 },
    fm_index: { type: "number", minimum: 0.0, maximum: 6.0 },
    fm_ratio: { type: "number", minimum: 0.5, maximum: 12.0 },
    loop_enabled: { type: "boolean" },
    partials: {
      type: "array",
      items: {
        type: "object",
        required: ["ratio", "amplitude", "decay_rate", "detune_cents", "pan"],
        properties: {
          ratio: { type: "number" },
          amplitude: { type: "number" },
          decay_rate: { type: "number" },
          detune_cents: { type: "number" },
          pan: { type: "number" }
        }
      }
    }
  }
};

function clamp(v: number, lo: number, hi: number): number {
  if (!Number.isFinite(v)) return lo;
  return Math.max(lo, Math.min(hi, v));
}

function sanitizePatchSpec(raw: Partial<AiSynthesizerPatchSpec>, prompt: string): AiSynthesizerPatchSpec {
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

async function generatePatchSpecWithWorkersAi(env: Env, prompt: string): Promise<AiSynthesizerPatchSpec> {
  const systemPrompt =
    "You are TEPUI Gran Sabana Sonic Lab's synthesizer patch & modal-spectral sample architect. " +
    "Given a user musical description, return ONLY a compact raw JSON object (no markdown fences) for a playable polyphonic Piano Roll instrument (NEVER speech or TTS) with these exact keys: " +
    '{"name":string,"category":string,"root_note":int(24..96),"attack":float(0.001..2.0),"decay":float(0.05..3.0),"sustain":float(0..1),"release":float(0.05..4.0),"cutoff":float(120..20000),"resonance":float(0.5..6.0),"filter_mode":int(0=LP,1=HP,2=BP),"lfo_rate":float(0.2..16),"lfo_depth":float(0..0.9),"lfo_target":int(0=Cutoff,1=Pitch,2=Amp),"arp_mode":int(0=Off,1=Up,2=Down,3=Joropo 6/8,4=Merengue 5/8),"space_mix":float(0..0.8),"mod_matrix_mask":int(0..33554431),"brightness":float(0..1),"exciter_noise":float(0..1),"body_warmth":float(0..1),"fm_index":float(0..4),"fm_ratio":float(0.5..8),"loop_enabled":bool,"partials":[{"ratio":float,"amplitude":float,"decay_rate":float,"detune_cents":float,"pan":float}]} (8 partials).';

  const rawResult = await env.AI.run(MODEL_ID, {
    messages: [
      { role: "system", content: systemPrompt },
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

function encodeStereoFloat32Wav(left: Float32Array, right: Float32Array, sampleRate: number): Uint8Array {
  const numFrames = Math.min(left.length, right.length);
  const numChannels = 2;
  const bytesPerSample = 4;
  const dataSize = numFrames * numChannels * bytesPerSample;
  const buffer = new ArrayBuffer(44 + dataSize);
  const view = new DataView(buffer);

  const writeStr = (offset: number, s: string) => {
    for (let i = 0; i < s.length; i++) {
      view.setUint8(offset + i, s.charCodeAt(i));
    }
  };

  writeStr(0, "RIFF");
  view.setUint32(4, 36 + dataSize, true);
  writeStr(8, "WAVE");
  writeStr(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 3, true); // IEEE Float32
  view.setUint16(22, numChannels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * numChannels * bytesPerSample, true);
  view.setUint16(32, numChannels * bytesPerSample, true);
  view.setUint16(34, 32, true);
  writeStr(36, "data");
  view.setUint32(40, dataSize, true);

  let offset = 44;
  for (let i = 0; i < numFrames; i++) {
    view.setFloat32(offset, left[i], true);
    view.setFloat32(offset + 4, right[i], true);
    offset += 8;
  }
  return new Uint8Array(buffer);
}

function synthesizeModalStereoWav(spec: AiSynthesizerPatchSpec, sampleRate: number): Uint8Array {
  const sr = clamp(sampleRate, 22050, 96000);
  const durSecs = spec.loop_enabled ? 2.4 : 1.6;
  const totalFrames = Math.floor(sr * durSecs);
  const left = new Float32Array(totalFrames);
  const right = new Float32Array(totalFrames);
  const f0 = 440.0 * Math.pow(2.0, (spec.root_note - 69.0) / 12.0);
  const attSecs = clamp(spec.attack * 0.25, 0.0005, 0.08);
  let rng = (spec.mod_matrix_mask ^ 0x9e3779b9) >>> 0;

  for (let i = 0; i < totalFrames; i++) {
    const t = i / sr;
    const envIn = t < attSecs ? t / attSecs : 1.0;
    const fmMod =
      spec.fm_index > 0.01
        ? spec.fm_index * Math.exp(-t * 3.5) * Math.sin(2.0 * Math.PI * f0 * spec.fm_ratio * t)
        : 0.0;

    let sumL = 0.0;
    let sumR = 0.0;
    for (let pIdx = 0; pIdx < spec.partials.length; pIdx++) {
      const p = spec.partials[pIdx];
      const detuneRatio = Math.pow(2.0, p.detune_cents / 1200.0);
      const freq = f0 * p.ratio * detuneRatio;
      if (freq >= sr * 0.46) continue;
      const phase = 2.0 * Math.PI * freq * t + fmMod / (1.0 + pIdx * 0.35);
      const env = Math.exp(-t * p.decay_rate);
      const sig = Math.sin(phase) * p.amplitude * env;
      const panAngle = (clamp(p.pan, -1.0, 1.0) + 1.0) * 0.25 * Math.PI;
      sumL += sig * Math.cos(panAngle);
      sumR += sig * Math.sin(panAngle);
    }

    rng = (Math.imul(rng, 1664525) + 1013904223) >>> 0;
    const white = (rng / 0xffffffff) * 2.0 - 1.0;
    const exciter = white * spec.exciter_noise * Math.exp(-t * 42.0) * 0.25;
    const body = Math.sin(2.0 * Math.PI * f0 * t) * spec.body_warmth * Math.exp(-t * 2.1) * 0.32;

    left[i] = Math.tanh((sumL + exciter + body) * envIn * 0.85);
    right[i] = Math.tanh((sumR - exciter * 0.6 + body) * envIn * 0.85);
  }

  return encodeStereoFloat32Wav(left, right, sr);
}

const CORS_HEADERS: Record<string, string> = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type, Authorization"
};

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }

    const url = new URL(request.url);
    const startMs = Date.now();

    try {
      if (url.pathname === "/health" || url.pathname === "/") {
        return new Response(
          JSON.stringify({
            ok: true,
            service: "tepui-ai-synth-gateway",
            model: MODEL_ID,
            engine: "CLOUDFLARE-WORKERS-AI"
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
        const spec = await generatePatchSpecWithWorkersAi(env, prompt);
        const elapsedMs = Date.now() - startMs;
        console.log(
          JSON.stringify({
            event: "preset_synthesized",
            prompt,
            root_note: spec.root_note,
            partials: spec.partials.length,
            elapsed_ms: elapsedMs
          })
        );
        return new Response(
          JSON.stringify({
            ok: true,
            backend: "CLOUDFLARE-WORKERS-AI",
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

      if (url.pathname === "/v1/sample.wav" && request.method === "POST") {
        const body = (await request.json().catch(() => ({}))) as {
          prompt?: string;
          sample_rate?: number;
        };
        const prompt = String(body.prompt || "Concert Grand Piano C4").trim().slice(0, 280);
        const sampleRate = clamp(Number(body.sample_rate || 48000), 22050, 96000);
        const spec = await generatePatchSpecWithWorkersAi(env, prompt);
        const wavBytes = synthesizeModalStereoWav(spec, sampleRate);
        return new Response(wavBytes, {
          status: 200,
          headers: {
            ...CORS_HEADERS,
            "Content-Type": "audio/wav",
            "X-Tepui-Backend": "CLOUDFLARE-WORKERS-AI",
            "X-Tepui-Root-Note": String(spec.root_note)
          }
        });
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
