// @ts-nocheck
import { arpStepSource, svfSpectrumSource, synthStore, vuMeterSource } from '../state/synthStore';
/**
 * TEPUI // GRAN SABANA SONIC LAB - STUDIO ENGINE CONTROLLER
 * Ultra-Low Latency Native IPC & Custom Protocol Bridge
 */

// Trap all unhandled errors and forward to native Rust console
window.onerror = function(msg, url, line, col, error) {
  var errStr = '[JS ERROR ' + line + ':' + col + '] ' + msg + (error ? ' ' + error.stack : '');
  console.error(errStr);
  if (typeof window.ipc !== 'undefined' && typeof window.ipc.postMessage === 'function') {
    try { window.ipc.postMessage('log:' + errStr); } catch (_) {}
  } else if (window.chrome && window.chrome.webview && typeof window.chrome.webview.postMessage === 'function') {
    try { window.chrome.webview.postMessage('log:' + errStr); } catch (_) {}
  }
};

window.onunhandledrejection = function(e) {
  var errStr = '[JS REJECTION] ' + (e.reason ? (e.reason.stack || e.reason) : e);
  console.error(errStr);
  if (typeof window.ipc !== 'undefined' && typeof window.ipc.postMessage === 'function') {
    try { window.ipc.postMessage('log:' + errStr); } catch (_) {}
  } else if (window.chrome && window.chrome.webview && typeof window.chrome.webview.postMessage === 'function') {
    try { window.chrome.webview.postMessage('log:' + errStr); } catch (_) {}
  }
};

try {
  if (typeof window.ipc !== 'undefined' && typeof window.ipc.postMessage === 'function') {
    window.ipc.postMessage('log:app.js parsed at top-level');
  } else if (window.chrome && window.chrome.webview && typeof window.chrome.webview.postMessage === 'function') {
    window.chrome.webview.postMessage('log:app.js parsed at top-level');
  }
} catch (_) {}

// -------------------------------------------------------------
// 1. ENGINE STATE & CONFIGURATION
// -------------------------------------------------------------
const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

function midiNoteToName(midi) {
  const m = Math.max(0, Math.min(127, Math.round(Number(midi) || 60)));
  const name = NOTE_NAMES[m % 12];
  const oct = Math.floor(m / 12) - 1;
  return `${name}${oct}`;
}

const state = {
  connected: false,
  usingNativeIpc: false,
  isGenerating: false,
  isDraggingKnob: false,
  isDraggingRegion: false,
  currentOctave: 3, // C3 (48) a C5 (72)
  currentVelocity: 0.95,
  activeVoices: 0,
  dspActiveVoices: 0,
  activeNotes: new Set(),
  vuLevelL: 0.0,
  vuLevelR: 0.0,
  dspPeakL: 0.0,
  dspPeakR: 0.0,
  playheadNorm: 0.0,
  lastParamRevision: 0,
  sampleName: 'Concert Grand Piano',

  params: {
    attack: 0.01,
    decay: 0.20,
    sustain: 0.80,
    release: 0.30,
    cutoff: 20000.0,
    resonance: 0.707,
    filter_mode: 0,
    gain: 0.0,
    space_mix: 0.15,
    root_note: 60,
    lfo_rate: 3.5,
    lfo_depth: 0.0,
    lfo_target: 0,
    lfo_shape: 0,
    arp_mode: 0,
    mod_matrix_mask: 2178,
    wt_pos: 0.25,
    wt_warp: 0.15,
    unison_voices: 1,
    unison_detune: 0.20,
    sub_level: 0.0,
    drive_amount: 0.0,
    ott_amount: 0.0,
    glide_time: 0.0,
    glide_legato: false,
    env2_depth: 0.35,
    env2_target: 0,
    env_curve: -0.65,
    arp_rate_div: 1,
    arp_step_count: 16,
    granular_mode: 0,
    macros: [0.0, 0.0, 0.0, 0.0]
  },
  normalized: {
    attack: 0.2702,
    decay: 0.622,
    sustain: 0.8,
    release: 0.6199,
    cutoff: 1.0,
    resonance: 0.06,
    gain: 0.909,
    space_mix: 0.15,
    root_note: 0.4724,
    lfo_rate: 0.32,
    lfo_depth: 0.0,
    wt_pos: 0.25,
    wt_warp: 0.15,
    unison_detune: 0.20,
    sub_level: 0.0,
    drive: 0.0,
    ott: 0.0,
    macro1: 0.0,
    macro2: 0.0,
    macro3: 0.0,
    macro4: 0.0
  },
  wt3dMode: true,
  region: {
    trim_start_norm: 0.0,
    trim_end_norm: 1.0,
    loop_enabled: true,
    loop_start_norm: 0.20,
    loop_end_norm: 0.85
  },
  oddMetrics: null,
  localUiPresetLoads: 0,
  localUiMatrixSwitches: 0,
  localUiResizeEvents: 0,
  localUiFavToggles: 0,
  localUserPresetsSaved: 0,
  pitchBendSemitones: 0.0,
  modWheelNorm: 0.0,
  sustainPedal: false,
  isDraggingWheel: false,
  lastPlayedVelocity: 0.95,
  localVelocityEvents: 0,
  localWavZoneDrops: 0,
  midiLearnArmedParam: null,
  midiCcAssignments: {},
  waveform: []
};

// -------------------------------------------------------------
// 2. ULTRA-LOW LATENCY IPC & WEBSOCKET ENGINE
// -------------------------------------------------------------
// 2. ULTRA-LOW LATENCY NATIVE IPC (100% SERVERLESS / IN-MEMORY)
// -------------------------------------------------------------

// Función universal e infalible para enviar IPC a Rust (cero excepciones)
function sendIpc(msg) {
  if (typeof window.ipc !== 'undefined' && window.ipc && typeof window.ipc.postMessage === 'function') {
    try {
      window.ipc.postMessage(msg);
      return true;
    } catch (_) {}
  }
  if (typeof window.chrome !== 'undefined' && window.chrome && window.chrome.webview && typeof window.chrome.webview.postMessage === 'function') {
    try {
      window.chrome.webview.postMessage(msg);
      return true;
    } catch (_) {}
  }
  return false;
}

// Polyfill de conveniencia
if (typeof window.ipc === 'undefined') {
  try {
    window.ipc = { postMessage: (msg) => sendIpc(msg) };
  } catch (_) {}
}

function hasNativeIpc() {
  return (typeof window.ipc !== 'undefined' && window.ipc && typeof window.ipc.postMessage === 'function')
      || (typeof window.chrome !== 'undefined' && window.chrome && window.chrome.webview && typeof window.chrome.webview.postMessage === 'function');
}

function updateEngineBadge() {
  const badge = document.getElementById('engine-badge');
  const text = document.getElementById('engine-text');
  if (!badge || !text) return;

  state.usingNativeIpc = true;
  badge.className = 'engine-badge engine-ipc';
  text.innerText = 'NATIVE MEMORY (0-LAG)';
}

// Envío ultrarrápido con bypass directo al hilo de audio de Rust
function sendFastNoteOn(midi, velocity) {
  const vel = Math.max(0.01, Math.min(1.0, Number(velocity) || state.currentVelocity || 0.95));
  state.lastPlayedVelocity = vel;
  state.localVelocityEvents = (state.localVelocityEvents || 0) + 1;
  state.activeNotes.add(midi);
  const velBadge = document.getElementById('vel-badge');
  if (velBadge) velBadge.innerText = `${Math.round(vel * 100)}%`;
  updateVoiceCount();
  sendIpc(`1,${midi},${vel.toFixed(3)}`);
  refreshFooterOddText();
}

function sendFastNoteOff(midi) {
  state.activeNotes.delete(midi);
  updateVoiceCount();
  sendIpc(`0,${midi}`);
}

function sendFastParam(paramId, normVal) {
  sendIpc(`p,${paramId},${normVal.toFixed(4)}`);
}

function sendFastFilterMode(mode) {
  sendIpc(`m,${mode}`);
}

function syncArpPatternToDsp(mode) {
  const m = Math.max(0, Math.min(4, Math.round(Number(mode) || 0)));
  const bars = getArpBarsForMode(m);
  if (Array.isArray(bars) && bars.length > 0) {
    const csv = bars.map((v) => Math.max(0.05, Math.min(1.0, Number(v) || 0.85)).toFixed(3)).join(',');
    sendIpc(`arpsteps,${csv}`);
  }
}

function sendFastArpMode(mode) {
  const m = Math.max(0, Math.min(4, Math.round(Number(mode) || 0)));
  sendIpc(`arp,${m}`);
  syncArpPatternToDsp(m);
}

function sendFastRootNote(note) {
  const clamped = Math.max(0, Math.min(127, Math.round(note)));
  state.params.root_note = clamped;
  state.normalized.root_note = clamped / 127.0;
  sendIpc(`r,${clamped}`);
  updateRootNoteDisplay(clamped);
}

function sendFastRegion() {
  const r = state.region;
  const en = r.loop_enabled ? 1 : 0;
  sendIpc(`l,${en},${r.trim_start_norm.toFixed(4)},${r.trim_end_norm.toFixed(4)},${r.loop_start_norm.toFixed(4)},${r.loop_end_norm.toFixed(4)}`);
  updateRegionDisplay();
}

function updateRegionDisplay(regionObj) {
  if (regionObj) {
    Object.assign(state.region, regionObj);
  }
  const r = state.region;
  const regEl = document.getElementById('telemetry-region');
  if (regEl) {
    const ts = Math.round((r.trim_start_norm || 0) * 100);
    const te = Math.round((r.trim_end_norm !== undefined ? r.trim_end_norm : 1) * 100);
    regEl.innerText = `TRIM: ${ts}%-${te}%`;
  }
  const loopBtn = document.getElementById('btn-loop-toggle');
  if (loopBtn) {
    loopBtn.classList.toggle('active', !!r.loop_enabled);
    loopBtn.innerText = r.loop_enabled ? 'LOOP:ON' : 'LOOP:OFF';
  }
}

function syncCloudCreditsBadgeUI(oddObj) {
  const odd = oddObj || state.oddMetrics || {};
  const tier = String(odd.cloud_tier || 'COMMUNITY KV').toUpperCase();
  const cr = odd.cloud_credits_remaining !== undefined ? odd.cloud_credits_remaining : 20;
  const dailyNeurons = odd.cloud_daily_neurons || 0;
  const isProTier = tier.includes('PRO');
  const isCustomProvider = tier.includes('OLLAMA') || tier.includes('GEMINI') || tier.includes('GROQ') || tier.includes('OPENAI') || tier.includes('BYOK') || isProTier;
  const isLocalOnly = tier.includes('LOCAL NPU');

  const btnCredits = document.getElementById('btn-cloud-credits');
  const txtCredits = document.getElementById('cloud-credits-text');
  if (btnCredits && txtCredits) {
    btnCredits.classList.toggle('tier-pro', isCustomProvider);
    if (tier.includes('OLLAMA')) {
      txtCredits.innerText = 'AI: OLLAMA LOCAL';
    } else if (tier.includes('GEMINI')) {
      txtCredits.innerText = 'AI: GEMINI';
    } else if (tier.includes('GROQ')) {
      txtCredits.innerText = 'AI: GROQ LPU';
    } else if (tier.includes('OPENAI')) {
      txtCredits.innerText = 'AI: OPENAI';
    } else if (isLocalOnly) {
      txtCredits.innerText = 'AI: NPU LOCAL';
    } else if (isProTier) {
      txtCredits.innerText = `PRO CLOUD (${cr} CR)`;
    } else if (cr < 5 || tier.includes('SIN CR')) {
      txtCredits.innerText = `SIN CRÉDITOS (${cr} CR)`;
    } else {
      txtCredits.innerText = `AI: KV (${cr} CR)`;
    }
  }

  const modalPill = document.getElementById('cloud-modal-tier-badge');
  if (modalPill) {
    modalPill.classList.toggle('tier-pro', isCustomProvider);
    if (isProTier) {
      modalPill.innerText = `PRO CLOUD • ${cr} CR`;
    } else if (isCustomProvider || isLocalOnly) {
      modalPill.innerText = tier;
    } else if (cr < 5 || tier.includes('SIN CR')) {
      modalPill.innerText = `SIN CRÉDITOS • ${cr} CR`;
    } else {
      modalPill.innerText = `COMMUNITY KV • ${cr} CR`;
    }
  }

  const quotaSum = document.getElementById('cloud-quota-summary');
  if (quotaSum) {
    quotaSum.innerText = `Pool Comunitario KV: ${dailyNeurons} / 9,500 Neurons hoy (Caché KV global = 0 Neurons en hits • Samples .WAV IA = 5 CR)`;
  }
}

function refreshFooterOddText() {
  const footerText = document.getElementById('footer-text');
  if (!footerText) return;
  const odd = state.oddMetrics || {};
  syncCloudCreditsBadgeUI(odd);
  const backend = (odd.synth_backend || 'HYBRID-PHYSICAL').toUpperCase();
  const durUs = odd.last_generation_duration_us || 0;
  const genCnt = odd.generation_count || 1;
  const blk = odd.dsp_blocks_processed || 0;
  const lim = odd.dsp_limiter_activations || 0;
  const lfoMod = Math.round(Math.abs(odd.lfo_mod_value || 0) * 100);
  const presets = Math.max(odd.ui_preset_loads || 0, state.localUiPresetLoads || 0);
  const matrix = Math.max(odd.ui_matrix_switches || 0, state.localUiMatrixSwitches || 0);
  const resizes = Math.max(odd.ui_resize_events || 0, state.localUiResizeEvents || 0);
  const favs = Math.max(odd.ui_fav_toggles || 0, state.localUiFavToggles || 0);
  const routes = odd.mod_matrix_active_routes !== undefined
    ? odd.mod_matrix_active_routes
    : countMaskBits(state.params.mod_matrix_mask || 0);
  const cacheHits = (odd.npu_cache_hits || 0) + (odd.cloud_kv_cache_hits || 0);
  const cacheMiss = odd.npu_cache_misses || 0;
  const cloudCr = odd.cloud_credits_remaining !== undefined ? odd.cloud_credits_remaining : 15;
  const liveVel = (typeof odd.last_velocity === 'number' && odd.last_velocity > 0.01)
    ? odd.last_velocity
    : (state.lastPlayedVelocity || state.currentVelocity || 0.95);
  const velPct = Math.round(Math.max(0.01, Math.min(1.0, liveVel)) * 100);
  const pitchHz = typeof odd.last_detected_pitch_hz === 'number' && odd.last_detected_pitch_hz > 10.0
    ? odd.last_detected_pitch_hz.toFixed(1)
    : (440.0 * Math.pow(2.0, ((state.params.root_note || 60) - 69) / 12.0)).toFixed(1);
  const aiPresets = odd.ai_presets_synthesized !== undefined ? odd.ai_presets_synthesized : genCnt;
  const glideCnt = odd.glide_transitions !== undefined ? odd.glide_transitions : (odd.glide_transitions_count || 0);
  const mpeCnt = odd.mpe_poly_events !== undefined ? odd.mpe_poly_events : (odd.mpe_poly_events_count || 0);
  const granFrames = odd.granular_dsp_frames || 0;
  const advFlt = odd.advanced_filter_frames || 0;
  const ratch = odd.arp_ratchet_hits !== undefined ? odd.arp_ratchet_hits : (odd.arp_ratchet_hits_count || 0);
  const ccMaps = odd.midi_learn_bindings_count !== undefined ? odd.midi_learn_bindings_count : (odd.midi_cc_learned_count || 0);
  footerText.innerText = `ODD // BACKEND: ${backend} • CLOUD: ${cloudCr}CR • SYNTH: ${durUs}µs (#${genCnt}) • AI-PATCH: #${aiPresets} • PITCH: ${pitchHz}Hz • CACHE: ${cacheHits}H/${cacheMiss}M • VEL: ${velPct}% • LFO: ${lfoMod}% • GLIDE: ${glideCnt} • MPE: ${mpeCnt} • GRAN: ${granFrames}f • FLT+: ${advFlt}f • RATCH: ${ratch} • CC-MAP: ${ccMaps} • ROUTES: ${routes} • PRESETS: ${presets} • FAVS: ${favs} • MATRIX: ${matrix} • RESIZE: ${resizes} • DSP BLK: ${blk} • LIM: ${lim}`;
}

function syncExpressionWheelsUI() {
  const pbCap = document.getElementById('wheel-pitch-cap');
  if (pbCap) {
    const normPb = Math.max(-1, Math.min(1, (state.pitchBendSemitones || 0) / 2.0));
    const topPx = Math.round(22 - normPb * 17);
    pbCap.style.top = `${topPx}px`;
  }
  const mwCap = document.getElementById('wheel-mod-cap');
  if (mwCap) {
    const normMw = Math.max(0, Math.min(1, state.modWheelNorm || 0));
    const botPx = Math.round(12 + normMw * 32);
    mwCap.style.bottom = `${botPx}px`;
  }
  const susBtn = document.getElementById('btn-sustain-pedal');
  if (susBtn) {
    susBtn.classList.toggle('active', !!state.sustainPedal);
  }
}

function countMaskBits(mask) {
  let m = (mask >>> 0) & 0x01ffffff;
  let c = 0;
  while (m) {
    c += m & 1;
    m >>>= 1;
  }
  return c;
}

function syncMatrixPinsFromMask(mask) {
  const cleanMask = (mask >>> 0) & 0x01ffffff;
  state.params.mod_matrix_mask = cleanMask;
  document.querySelectorAll('.mod-matrix-mini .matrix-row').forEach((rowEl, rowIdx) => {
    const pins = rowEl.querySelectorAll('i');
    pins.forEach((pinEl, colIdx) => {
      const bit = 1 << (rowIdx * 5 + colIdx);
      pinEl.classList.toggle('on', (cleanMask & bit) !== 0);
    });
  });
}

function updateOddMetricsDisplay(odd) {
  if (!odd) return;
  state.oddMetrics = odd;
  const mask = odd.active_keyzones_mask || 0;
  const zLowBtn = document.getElementById('btn-zone-low');
  const zMidBtn = document.getElementById('btn-zone-mid');
  const zHighBtn = document.getElementById('btn-zone-high');
  if (zLowBtn) zLowBtn.classList.toggle('active', (mask & 1) !== 0);
  if (zMidBtn) zMidBtn.classList.toggle('active', (mask & 2) !== 0);
  if (zHighBtn) zHighBtn.classList.toggle('active', (mask & 4) !== 0);

  const zonesVal = document.getElementById('t-zones-val');
  if (zonesVal) {
    if (mask === 0) {
      zonesVal.innerText = 'GLOBAL';
    } else {
      const parts = [];
      if (mask & 1) parts.push('L');
      if (mask & 2) parts.push('M');
      if (mask & 4) parts.push('H');
      zonesVal.innerText = `SPLIT (${parts.join('+')})`;
    }
  }

  const recBtn = document.getElementById('btn-live-rec');
  if (recBtn) {
    if (odd.is_recording_master) {
      const secs = ((odd.recorded_master_frames || 0) / 48000).toFixed(1);
      recBtn.classList.add('active', 'panic-btn');
      recBtn.innerText = `■ STOP (${secs}s)`;
    } else {
      recBtn.classList.remove('active', 'panic-btn');
      recBtn.innerText = '● REC LIVE';
    }
  }

  const arpStepsEl = document.getElementById('arp-steps-count');
  if (arpStepsEl && odd.arp_steps_triggered !== undefined) {
    arpStepsEl.innerText = String(odd.arp_steps_triggered);
  }

  if (
    (odd.midi_learn_armed_param === undefined || odd.midi_learn_armed_param < 0) &&
    typeof odd.last_learned_cc === 'number' &&
    odd.last_learned_cc >= 0 &&
    state.midiLearnArmedParam
  ) {
    const learnedParam = state.midiLearnArmedParam;
    state.midiCcAssignments[learnedParam] = odd.last_learned_cc;
    state.midiLearnArmedParam = null;
    syncMidiLearnUI();
  }

  if (!state.isDraggingWheel) {
    if (typeof odd.pitch_bend_semitones === 'number') {
      state.pitchBendSemitones = odd.pitch_bend_semitones;
    }
    if (typeof odd.mod_wheel === 'number') {
      state.modWheelNorm = odd.mod_wheel;
    }
    if (typeof odd.sustain_pedal === 'boolean') {
      state.sustainPedal = odd.sustain_pedal;
    }
    syncExpressionWheelsUI();
  }

  refreshFooterOddText();
}

function updateRootNoteDisplay(rootNote, sampleName) {
  if (sampleName) state.sampleName = sampleName;
  const root = Math.max(0, Math.min(127, Math.round(rootNote !== undefined ? rootNote : state.params.root_note)));
  state.params.root_note = root;
  const noteLabel = `${midiNoteToName(root)} (${root})`;
  const rootValEl = document.getElementById('t-root-val');
  if (rootValEl) rootValEl.innerText = noteLabel;
  const telemRootEl = document.getElementById('telemetry-root');
  if (telemRootEl) {
    const upperName = (state.sampleName || 'CUSTOM SAMPLE').toUpperCase();
    telemRootEl.innerText = `${upperName} • ${midiNoteToName(root)}`;
  }
}

let pollTimer = null;
function pollAiGeneration() {
  if (pollTimer) clearInterval(pollTimer);
  pollTimer = setInterval(async () => {
    try {
      const resp = await fetch('/api/init-state');
      if (resp.ok) {
        const data = await resp.json();
        if (!data.is_generating) {
          clearInterval(pollTimer);
          pollTimer = null;
          state.isGenerating = false;
          updateGeneratingUI(false);
          if (data.params) {
            Object.assign(state.params, data.params);
            syncVitalSerumParamsToUI(data.params);
          }
          if (data.normalized) Object.assign(state.normalized, data.normalized);
          if (data.param_revision !== undefined) state.lastParamRevision = data.param_revision;
          if (data.waveform && data.waveform.length > 0) {
            state.waveform = data.waveform;
          }
          if (state.params.mod_matrix_mask !== undefined && state.params.mod_matrix_mask > 0) {
            syncMatrixPinsFromMask(state.params.mod_matrix_mask);
          }
          updateAllKnobsUI();
          updateFilterModeUI(state.params.filter_mode);
          updateLfoTargetUI(state.params.lfo_target || 0);
          updateArpModeUI(state.params.arp_mode || 0);
          if (data.sample_info) updateTelemetry(data.sample_info);
          if (data.region && !state.isDraggingRegion) updateRegionDisplay(data.region);
          if (data.odd_metrics) updateOddMetricsDisplay(data.odd_metrics);
          if (typeof markAllCanvasesDirty === 'function') markAllCanvasesDirty();
          drawOscilloscope();
          const genBtn = document.getElementById('btn-generate');
          const backendName = (data.odd_metrics && data.odd_metrics.synth_backend)
            ? String(data.odd_metrics.synth_backend).toUpperCase()
            : 'NEURAL-SPECTRAL-DSP';
          if (genBtn && typeof showParamHudTooltip === 'function') {
            showParamHudTooltip(genBtn, backendName, data.sample_info?.name || 'PATCH LISTO');
            setTimeout(hideParamHudTooltip, 1100);
          }
          if (state.activeNotes.size === 0) {
            const auditionMidi = (data.sample_info && data.sample_info.root_note) || state.params.root_note || 60;
            pressNote(auditionMidi, state.currentVelocity || 0.90);
            setTimeout(() => releaseNote(auditionMidi), 260);
          }
        }
      }
    } catch (e) {
      console.warn('[AI Poll Error]', e);
    }
  }, 150);
}

function sendFastGenerate(prompt) {
  sendIpc(`gen:${prompt}`);
  state.isGenerating = true;
  updateGeneratingUI(true);
  pollAiGeneration();
}

function syncVitalSerumParamsToUI(p) {
  if (!p) return;
  if (typeof p.wt_pos === 'number') state.normalized.wt_pos = p.wt_pos;
  if (typeof p.wt_warp === 'number') state.normalized.wt_warp = p.wt_warp;
  if (typeof p.unison_detune === 'number') state.normalized.unison_detune = p.unison_detune;
  if (typeof p.sub_level === 'number') state.normalized.sub_level = p.sub_level;
  if (typeof p.drive_amount === 'number') state.normalized.drive = p.drive_amount;
  if (typeof p.ott_amount === 'number') state.normalized.ott = p.ott_amount;
  if (Array.isArray(p.macros) && p.macros.length >= 4) {
    state.normalized.macro1 = p.macros[0] || 0;
    state.normalized.macro2 = p.macros[1] || 0;
    state.normalized.macro3 = p.macros[2] || 0;
    state.normalized.macro4 = p.macros[3] || 0;
  }
  if (typeof p.unison_voices === 'number') updateUnisonVoicesUI(p.unison_voices);
  if (typeof p.fm_from_b === 'number') updateFmFromBUI(p.fm_from_b);
  if (typeof p.lfo_shape === 'number') updateLfoShapeUI(p.lfo_shape);
  if (typeof p.granular_mode === 'number') updateGranularModeUI(p.granular_mode);
  if (typeof p.env2_target === 'number' || typeof p.env2_depth === 'number') {
    updateEnv2UI(p.env2_target !== undefined ? p.env2_target : state.params.env2_target, p.env2_depth !== undefined ? p.env2_depth : state.params.env2_depth);
  }
  if (typeof p.env_curve === 'number') updateEnvCurveUI(p.env_curve);
  if (typeof p.arp_rate_div === 'number') updateArpRateUI(p.arp_rate_div);
  if (typeof p.arp_step_count === 'number') updateArpStepsCountUI(p.arp_step_count);
  if (typeof p.glide_time === 'number' || typeof p.glide_legato === 'boolean') {
    updateGlideUI(p.glide_time || 0, !!p.glide_legato);
  } else if (typeof p.glide_mode === 'number') {
    if (p.glide_mode === 2) {
      updateGlideUI(0.160, true);
    } else if (p.glide_mode === 1) {
      updateGlideUI(0.085, false);
    } else {
      updateGlideUI(0, false);
    }
  }
  if (typeof p.filter_keytrack === 'number') {
    const ktBtn = document.getElementById('btn-svf-keytrack');
    if (ktBtn) {
      if (p.filter_keytrack >= 0.75) {
        ktBtn.dataset.kt = '100';
        ktBtn.innerText = 'KEY 100%';
        ktBtn.classList.add('active');
      } else if (p.filter_keytrack >= 0.25) {
        ktBtn.dataset.kt = '50';
        ktBtn.innerText = 'KEY 50%';
        ktBtn.classList.add('active');
      } else {
        ktBtn.dataset.kt = '0';
        ktBtn.innerText = 'KEY OFF';
        ktBtn.classList.remove('active');
      }
    }
  }
  if (p.arp_custom_accents_active && Array.isArray(p.arp_step_accents) && p.arp_step_accents.length >= 12 && !state.isDraggingArpOled) {
    const curMode = state.params.arp_mode || 0;
    const stepLen = state.params.arp_step_count === 16 ? 16 : 12;
    const cleaned = p.arp_step_accents.slice(0, stepLen).map((v) => Math.max(0.05, Math.min(1.0, Number(v) || 0.85)));
    state.arpCustomBars = cleaned;
    if (!state.arpCustomPatterns) state.arpCustomPatterns = {};
    state.arpCustomPatterns[curMode] = [...cleaned];
  }
  if (Array.isArray(p.midi_cc_map)) {
    const IDX_TO_PARAM = [
      'cutoff', 'resonance', 'attack', 'decay', 'sustain', 'release',
      'wt_pos', 'wt_warp', 'unison_detune', 'sub_level', 'lfo_rate', 'lfo_depth',
      'drive', 'ott', 'space_mix', 'gain', 'macro1', 'macro2', 'macro3', 'macro4'
    ];
    const nextMap = {};
    p.midi_cc_map.forEach((pair) => {
      if (Array.isArray(pair) && pair.length >= 2) {
        const cc = Number(pair[0]);
        const pIdx = Number(pair[1]);
        const pName = IDX_TO_PARAM[pIdx];
        if (pName && cc >= 0 && cc < 128) {
          nextMap[pName] = cc;
        }
      }
    });
    state.midiCcAssignments = nextMap;
    syncMidiLearnUI();
  }
}

function applyInitPayload(msg) {
  if (!msg) return;
  if (msg.params) {
    Object.assign(state.params, msg.params);
    syncVitalSerumParamsToUI(msg.params);
  }
  if (msg.normalized) Object.assign(state.normalized, msg.normalized);
  if (msg.param_revision !== undefined) state.lastParamRevision = msg.param_revision;
  if (msg.peak_l !== undefined) state.dspPeakL = msg.peak_l;
  if (msg.peak_r !== undefined) state.dspPeakR = msg.peak_r;
  if (msg.active_voices !== undefined) state.dspActiveVoices = msg.active_voices;
  if (typeof msg.playhead_norm === 'number') state.playheadNorm = msg.playhead_norm;
  if (msg.waveform && msg.waveform.length > 0) {
    state.waveform = msg.waveform;
  }
  if (msg.sample_info) updateTelemetry(msg.sample_info);
  if (msg.region) updateRegionDisplay(msg.region);
  if (msg.odd_metrics) updateOddMetricsDisplay(msg.odd_metrics);
  if (Array.isArray(msg.favorites) && msg.favorites.length > 0) {
    msg.favorites.forEach((fid) => favoritePresets.add(fid));
  }
  if (Array.isArray(msg.user_presets) && msg.user_presets.length > 0) {
    buildPresetCatalog();
    msg.user_presets.forEach((up) => {
      if (up && up.id && !PRESET_CATALOG.some((p) => p.id === up.id)) {
        PRESET_CATALOG.unshift(up);
      }
    });
  }
  if (state.params.mod_matrix_mask !== undefined && state.params.mod_matrix_mask > 0) {
    syncMatrixPinsFromMask(state.params.mod_matrix_mask);
  }
  state.isGenerating = msg.is_generating || false;
  updateAllKnobsUI();
  updateFilterModeUI(state.params.filter_mode);
  updateLfoTargetUI(state.params.lfo_target || 0);
  updateArpModeUI(state.params.arp_mode || 0);
  updateVoiceCount();
  drawOscilloscope();
  state.connected = true;
  updateEngineBadge();
}

async function fetchInitState() {
  if (window.__TEPUI_INIT_STATE__) {
    applyInitPayload(window.__TEPUI_INIT_STATE__);
    return;
  }
  try {
    const resp = await fetch('/api/init-state');
    if (resp.ok) {
      const msg = await resp.json();
      applyInitPayload(msg);
    }
  } catch (e) {
    console.warn('[TEPUI INIT STATE FETCH ERROR]', e);
  }
}

let telemetryTimer = null;
function startTelemetryLoop() {
  if (telemetryTimer) clearInterval(telemetryTimer);
  telemetryTimer = setInterval(async () => {
    try {
      const resp = await fetch('/api/telemetry');
      if (!resp.ok) return;
      const data = await resp.json();
      if (typeof data.peak_l === 'number') state.dspPeakL = data.peak_l;
      if (typeof data.peak_r === 'number') state.dspPeakR = data.peak_r;
      if (typeof data.playhead_norm === 'number') state.playheadNorm = data.playhead_norm;
      if (typeof data.active_voices === 'number') {
        state.dspActiveVoices = data.active_voices;
        updateVoiceCount();
      }
      if (data.odd_metrics) {
        updateOddMetricsDisplay(data.odd_metrics);
      }
      if (data.param_revision !== undefined && data.param_revision !== state.lastParamRevision) {
        state.lastParamRevision = data.param_revision;
        if (!state.isDraggingKnob && !state.isDraggingRegion) {
          if (data.params) {
            Object.assign(state.params, data.params);
            syncVitalSerumParamsToUI(data.params);
          }
          if (data.normalized) Object.assign(state.normalized, data.normalized);
          if (state.params.mod_matrix_mask !== undefined && state.params.mod_matrix_mask > 0) {
            syncMatrixPinsFromMask(state.params.mod_matrix_mask);
          }
          updateAllKnobsUI();
          updateFilterModeUI(state.params.filter_mode);
          updateLfoTargetUI(state.params.lfo_target || 0);
          updateArpModeUI(state.params.arp_mode || 0);
          if (data.sample_info) updateTelemetry(data.sample_info);
          if (data.region) updateRegionDisplay(data.region);
        }
      }
    } catch (_) {}
  }, 80);
}

// -------------------------------------------------------------
// 3. VOICE COUNTER & HARDWARE LED MONITOR
// -------------------------------------------------------------
function updateVoiceCount() {
  const count = Math.min(16, Math.max(state.activeNotes.size, state.dspActiveVoices || 0));
  state.activeVoices = count;

  const countEl = document.getElementById('voice-count');
  if (countEl) countEl.innerText = count;

  const leds = document.querySelectorAll('#voice-leds i');
  leds.forEach((led, idx) => {
    led.classList.toggle('on', idx < count);
  });
}

// -------------------------------------------------------------
// 4. ROTARY KNOBS WIDGET (SVG ARC + PRECISION MODIFIERS)
// -------------------------------------------------------------
const KNOB_CONFIG = {
  radius: 18,
  circumference: 2 * Math.PI * 18, // ~ 113.1
  arcAngle: 270,
  startAngle: 135
};

let paramHudTimer = null;

function showParamHudTooltip(anchorEl, paramName, formattedVal) {
  const hud = document.getElementById('param-hud-tooltip');
  if (!hud || !anchorEl) return;
  const rect = anchorEl.getBoundingClientRect();
  hud.innerHTML = `<span class="hud-param-name">${paramName}</span><span class="hud-param-val">${formattedVal}</span>`;
  hud.style.left = `${Math.round(rect.left + rect.width * 0.5)}px`;
  hud.style.top = `${Math.max(24, Math.round(rect.top - 6))}px`;
  hud.classList.remove('hidden');
  if (paramHudTimer) clearTimeout(paramHudTimer);
  paramHudTimer = setTimeout(hideParamHudTooltip, 900);
}

function hideParamHudTooltip() {
  if (paramHudTimer) {
    clearTimeout(paramHudTimer);
    paramHudTimer = null;
  }
  const hud = document.getElementById('param-hud-tooltip');
  if (hud) hud.classList.add('hidden');
}

function initKnobs() {
  window.__tepuiOnParamChange = (paramId, newNorm, rootEl) => {
    const clamped = Math.max(0.0, Math.min(1.0, Number(newNorm) || 0.0));
    state.isDraggingKnob = true;
    state.normalized[paramId] = clamped;
    updateKnobUI(paramId, clamped);
    const anchor = rootEl || document.querySelector(`.knob-container[data-param="${paramId}"]`);
    if (anchor) {
      const labelEl = anchor.querySelector('.knob-label');
      const paramTitle = (labelEl ? labelEl.innerText : paramId).toUpperCase();
      showParamHudTooltip(anchor, paramTitle, formatKnobValue(paramId, clamped));
    }
    sendFastParam(paramId, clamped);
  };

  window.__tepuiOnParamCommit = (_paramId, _newNorm) => {
    state.isDraggingKnob = false;
    hideParamHudTooltip();
  };

  window.__tepuiOnKnobContextMenu = (paramId, e) => {
    if (e && e.shiftKey) return;
    if (e && typeof e.preventDefault === 'function') {
      e.preventDefault();
      e.stopPropagation();
    }
    const container = document.querySelector(`.knob-container[data-param="${paramId}"]`);
    const labelEl = container ? container.querySelector('.knob-label') : null;
    const paramTitle = (labelEl ? labelEl.innerText : paramId).toUpperCase();
    if (state.midiLearnArmedParam === paramId) {
      state.midiLearnArmedParam = null;
      sendIpc('midi_learn:off');
    } else {
      state.midiLearnArmedParam = paramId;
      sendIpc(`midi_learn:${paramId}`);
      if (container) {
        showParamHudTooltip(container, `MIDI LEARN → ${paramTitle}`, 'MUEVE UN CC FÍSICO (0..127)');
        setTimeout(hideParamHudTooltip, 1300);
      }
    }
    syncMidiLearnUI();
  };
}

function syncMidiLearnUI() {
  const armed = state.midiLearnArmedParam;
  document.querySelectorAll('.knob-container').forEach((container) => {
    const pid = container.dataset.param;
    container.classList.toggle('midi-learn-armed', armed === pid);
    let badge = container.querySelector('.midi-cc-badge');
    const assignedCc = state.midiCcAssignments && state.midiCcAssignments[pid];
    if (assignedCc !== undefined && assignedCc >= 0) {
      if (!badge) {
        badge = document.createElement('span');
        badge.className = 'midi-cc-badge';
        container.appendChild(badge);
      }
      badge.innerText = `CC#${assignedCc}`;
    } else if (badge) {
      badge.remove();
    }
  });
  const btnLearn = document.getElementById('btn-midi-learn');
  if (btnLearn) {
    btnLearn.classList.toggle('active', !!armed);
    btnLearn.innerText = armed ? `LEARN: ${armed.toUpperCase()}` : 'MIDI LEARN';
  }
}

function getAccentClass(id) {
  if (['attack', 'decay', 'sustain', 'release', 'wt_pos', 'macro1', 'macro4'].includes(id)) return 'cyan-accent';
  if (['cutoff', 'resonance', 'wt_warp', 'drive', 'macro2'].includes(id)) return 'amber-accent';
  return 'green-accent';
}

function getAccentColor(id) {
  const skin = (typeof currentUiSkin === 'string' && currentUiSkin) ? currentUiSkin : 'minimal';
  if (skin === 'minimal') {
    if (['cutoff', 'resonance', 'wt_warp', 'drive', 'macro2'].includes(id)) return '#17181a';
    return '#e03c1f';
  }
  if (skin === 'retro') {
    if (['cutoff', 'resonance', 'wt_warp', 'drive', 'macro2'].includes(id)) return '#231f1c';
    return '#c84b31';
  }
  if (skin === 'synth70') {
    if (['cutoff', 'resonance', 'wt_warp', 'drive', 'macro2'].includes(id)) return '#eab308';
    return '#e06d53';
  }
  if (skin === 'gunmetal') {
    if (['cutoff', 'resonance', 'wt_warp', 'drive', 'macro2'].includes(id)) return '#38bdf8';
    return '#f59e0b';
  }
  if (skin === 'phosphor') {
    return '#39ff88';
  }
  if (['attack', 'decay', 'sustain', 'release', 'wt_pos', 'macro1', 'macro4'].includes(id)) return '#00ebff';
  if (['cutoff', 'resonance', 'wt_warp', 'drive', 'macro2'].includes(id)) return '#ffaa00';
  return '#10b981';
}

function updateKnobUI(paramId, norm, _displayVal) {
  const clamped = Math.max(0.0, Math.min(1.0, Number.isFinite(norm) ? norm : 0.0));
  synthStore.setParamNormalized(paramId, clamped);
}

function formatKnobValue(id, norm) {
  if (id === 'attack' || id === 'decay') {
    const val = 0.001 * Math.pow(5.0 / 0.001, norm);
    return val >= 1.0 ? `${val.toFixed(2)} s` : `${(val * 1000).toFixed(0)} ms`;
  }
  if (id === 'sustain') {
    return `${(norm * 100).toFixed(0)} %`;
  }
  if (id === 'release') {
    const val = 0.001 * Math.pow(10.0 / 0.001, norm);
    return val >= 1.0 ? `${val.toFixed(2)} s` : `${(val * 1000).toFixed(0)} ms`;
  }
  if (id === 'cutoff') {
    const val = 20.0 * Math.pow(20000.0 / 20.0, norm);
    return val >= 1000 ? `${(val / 1000).toFixed(1)} kHz` : `${Math.round(val)} Hz`;
  }
  if (id === 'resonance') {
    return (0.1 + norm * (10.0 - 0.1)).toFixed(2);
  }
  if (id === 'gain') {
    const db = -60.0 + norm * 66.0;
    return `${db.toFixed(1)} dB`;
  }
  if (id === 'space_mix' || id === 'lfo_depth' || ['wt_pos', 'wt_warp', 'unison_detune', 'sub_level', 'drive', 'ott', 'macro1', 'macro2', 'macro3', 'macro4'].includes(id)) {
    return `${Math.round(norm * 100)} %`;
  }
  if (id === 'lfo_rate') {
    const val = 0.1 * Math.pow(20.0 / 0.1, norm);
    return `${val.toFixed(1)} Hz`;
  }
  return norm.toFixed(2);
}

const LFO_TARGET_LABELS = ['LFO → CUTOFF', 'LFO → VIBRATO', 'LFO → TREMOLO', 'LFO → OFF'];
const LFO_SHAPE_LABELS = ['SINE', 'TRI', 'SAW', 'RAMP', 'SQR', 'S&H'];
const GRANULAR_MODE_LABELS = ['GRAN:OFF', 'GRAN:CLOUD', 'GRAN:FREEZE'];
const ARP_RATE_LABELS = ['1/4', '1/8', '1/16', '1/8T', '1/16D'];

function updateGranularModeUI(granMode) {
  const g = Math.max(0, Math.min(2, Math.round(Number(granMode) || 0)));
  state.params.granular_mode = g;
  const btn = document.getElementById('btn-granular-mode');
  if (btn) {
    btn.innerText = GRANULAR_MODE_LABELS[g] || 'GRAN:OFF';
    btn.classList.toggle('active', g > 0);
  }
}

function updateEnv2UI(targetInt, depthVal) {
  const t = Math.max(0, Math.min(3, Math.round(Number(targetInt) || 0)));
  const d = typeof depthVal === 'number' ? depthVal : (state.params.env2_depth || 0.35);
  state.params.env2_target = t;
  state.params.env2_depth = d;
  const btn = document.getElementById('btn-env2-target');
  if (btn) {
    if (Math.abs(d) < 0.02 || t === 3) {
      btn.innerText = 'ENV2:OFF';
      btn.classList.remove('active');
    } else {
      const names = ['ENV2→CUT', 'ENV2→PIT', 'ENV2→WT'];
      btn.innerText = names[t] || 'ENV2→CUT';
      btn.classList.add('active');
    }
  }
}

function updateEnvCurveUI(curveVal) {
  const c = Math.max(-1.0, Math.min(1.0, Number(curveVal) || 0.0));
  state.params.env_curve = c;
  const btn = document.getElementById('btn-env-curve');
  if (btn) {
    if (c < -0.2) {
      btn.innerText = 'CRV:EXP';
      btn.classList.add('active');
    } else if (c > 0.2) {
      btn.innerText = 'CRV:LOG';
      btn.classList.add('active');
    } else {
      btn.innerText = 'CRV:LIN';
      btn.classList.remove('active');
    }
  }
}

function updateArpRateUI(rateIdx) {
  const r = Math.max(0, Math.min(4, Math.round(Number(rateIdx) || 1)));
  state.params.arp_rate_div = r;
  const btn = document.getElementById('btn-arp-rate');
  if (btn) {
    btn.innerText = ARP_RATE_LABELS[r] || '1/8';
    btn.classList.toggle('active', r !== 1);
  }
}

function updateArpStepsCountUI(stepsCount) {
  const sc = Math.round(Number(stepsCount) || 16) >= 16 ? 16 : 12;
  state.params.arp_step_count = sc;
  const btn = document.getElementById('btn-arp-steps');
  if (btn) {
    btn.innerText = `${sc} STEPS`;
    btn.classList.toggle('active', sc === 16);
  }
  synthStore.setArpSteps(getArpBarsForMode(state.params.arp_mode || 0));
}

function updateGlideUI(glideTimeS, legatoOn) {
  const gt = Math.max(0.0, Math.min(2.0, Number(glideTimeS) || 0.0));
  const leg = !!legatoOn;
  state.params.glide_time = gt;
  state.params.glide_legato = leg;
  const btn = document.getElementById('btn-glide-legato');
  if (btn) {
    if (gt <= 0.002) {
      btn.innerText = 'GLIDE:OFF';
      btn.classList.remove('active');
    } else if (leg) {
      btn.innerText = `LEGATO ${Math.round(gt * 1000)}ms`;
      btn.classList.add('active');
    } else {
      btn.innerText = `PORTA ${Math.round(gt * 1000)}ms`;
      btn.classList.add('active');
    }
  }
}

function updateLfoTargetUI(targetInt) {
  const t = Math.max(0, Math.min(3, Math.round(Number(targetInt) || 0)));
  state.params.lfo_target = t;
  const btn = document.getElementById('btn-lfo-target');
  if (btn) {
    btn.innerText = LFO_TARGET_LABELS[t] || 'LFO → CUTOFF';
  }
}

function updateLfoShapeUI(shapeInt) {
  const s = Math.max(0, Math.min(5, Math.round(Number(shapeInt) || 0)));
  state.params.lfo_shape = s;
  const btn = document.getElementById('btn-lfo-shape');
  if (btn) {
    btn.innerText = LFO_SHAPE_LABELS[s] || 'SINE';
  }
}

function updateUnisonVoicesUI(voicesInt) {
  const v = Math.max(1, Math.min(8, Math.round(Number(voicesInt) || 1)));
  state.params.unison_voices = v;
  const btn = document.getElementById('btn-unison-voices');
  if (btn) {
    btn.innerText = `UNI:${v}V`;
    btn.classList.toggle('active', v > 1);
  }
}

function updateFmFromBUI(fmVal) {
  const f = Math.max(0.0, Math.min(1.0, Number(fmVal) || 0.0));
  state.params.fm_from_b = f;
  const btn = document.getElementById('btn-fm-b') || document.getElementById('btn-fm-b-toggle');
  if (btn) {
    const pct = Math.round(f * 100);
    btn.innerText = `FM B:${pct}%`;
    btn.classList.toggle('active', pct > 0);
  }
}

function updateAllKnobsUI() {
  const bgStrokeMap = {
    minimal: 'rgba(23, 24, 26, 0.18)',
    retro: 'rgba(35, 31, 28, 0.20)',
    synth70: 'rgba(244, 232, 193, 0.16)',
    gunmetal: 'rgba(148, 163, 184, 0.18)',
    phosphor: 'rgba(57, 255, 136, 0.18)',
    dark: '#161e2b',
  };
  const bgTrackStroke = bgStrokeMap[currentUiSkin] || '#161e2b';
  document.querySelectorAll('.knob-container .knob-bg-track').forEach((circle) => {
    circle.setAttribute('stroke', bgTrackStroke);
  });

  document.querySelectorAll('.knob-container').forEach((el) => {
    const paramId = el.dataset.param;
    if (paramId) {
      updateKnobUI(
        paramId,
        state.normalized && state.normalized[paramId] !== undefined
          ? state.normalized[paramId]
          : 0.5
      );
    }
  });
}

function updateLiveKnobModulationRings() {
  const depth = state.normalized.lfo_depth !== undefined ? state.normalized.lfo_depth : (state.params.lfo_depth || 0);
  const target = state.params.lfo_target !== undefined ? state.params.lfo_target : 0;
  const lfoWave = (state.oddMetrics && typeof state.oddMetrics.lfo_mod_value === 'number' && Math.abs(state.oddMetrics.lfo_mod_value) > 0.001)
    ? state.oddMetrics.lfo_mod_value
    : Math.sin(animPhase) * depth;

  const targetParamMap = { 0: 'cutoff', 1: 'lfo_rate', 2: 'gain' };
  const activeModParam = target !== 3 ? targetParamMap[target] : null;
  const modCircum = 2 * Math.PI * 20.6;
  const maxModArc = (270 / 360) * modCircum;

  document.querySelectorAll('.knob-container').forEach((container) => {
    const pid = container.dataset.param;
    const modCircle = container.querySelector('.knob-mod-track');
    if (!modCircle) return;
    if (pid === activeModParam && depth > 0.01) {
      const baseNorm = state.normalized[pid] !== undefined ? state.normalized[pid] : 0.5;
      const modNorm = Math.max(0.0, Math.min(1.0, baseNorm + lfoWave * 0.28));
      const arcLen = modNorm * maxModArc;
      modCircle.setAttribute('stroke-dasharray', `${arcLen.toFixed(1)} ${modCircum.toFixed(1)}`);
      modCircle.setAttribute('opacity', '0.9');
    } else {
      modCircle.setAttribute('opacity', '0');
    }
  });
}

// -------------------------------------------------------------
// 5. FILTER MODE & ADVANCED FILTER MODEL SWITCHER
// -------------------------------------------------------------
function initFilterModes() {
  document.querySelectorAll('.mode-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      const mode = parseInt(btn.dataset.mode, 10);
      state.params.filter_mode = mode;
      updateFilterModeUI(mode);
      sendFastFilterMode(mode);
    });
  });

  const modelBtn = document.getElementById('btn-svf-model');
  if (modelBtn) {
    const MODEL_CYCLE = [0, 4, 5, 6];
    modelBtn.addEventListener('click', () => {
      const cur = state.params.filter_mode || 0;
      const curBase = cur >= 4 ? cur : 0;
      const idx = MODEL_CYCLE.indexOf(curBase);
      const next = MODEL_CYCLE[(idx + 1) % MODEL_CYCLE.length];
      state.params.filter_mode = next;
      updateFilterModeUI(next);
      sendFastFilterMode(next);
    });
  }
}

function updateFilterModeUI(mode) {
  const m = Math.max(0, Math.min(6, Math.round(Number(mode) || 0)));
  state.params.filter_mode = m;
  synthStore.setFilterMode(m);
  document.querySelectorAll('.mode-btn').forEach((btn) => {
    const btnMode = parseInt(btn.dataset.mode, 10);
    btn.classList.toggle('active', btnMode === m);
  });
  const modelBtn = document.getElementById('btn-svf-model');
  const slopeBadge = document.getElementById('svf-slope-badge');
  if (modelBtn) {
    const labels = {
      0: 'MODEL:SVF',
      1: 'MODEL:SVF',
      2: 'MODEL:SVF',
      3: 'MODEL:BYP',
      4: 'LADDER 24',
      5: 'COMB K-S',
      6: 'FORMANT'
    };
    modelBtn.innerText = labels[m] || 'MODEL:SVF';
    modelBtn.classList.toggle('active', m >= 4);
  }
  if (slopeBadge) {
    const slopes = {
      0: '12/24 dB',
      1: '12/24 dB',
      2: '12 dB BP',
      3: 'BYPASS',
      4: 'MOOG 24dB',
      5: 'COMB DLY',
      6: 'A-E-I-O-U'
    };
    slopeBadge.innerText = slopes[m] || '12/24 dB';
  }
}

// -------------------------------------------------------------
// 6. REAL-TIME OLED OSCILLOSCOPE, INTERACTIVE TRIM/LOOP & STEREO VU METERS
// -------------------------------------------------------------
let animFrameId = null;
let animPhase = 0;
let activeRegionMarker = null; // 'trim_start' | 'trim_end' | 'loop_start' | 'loop_end'

function initOscilloscope() {
  if (state.waveform.length === 0) {
    state.waveform = [];
    for (let i = 0; i < 360; i++) {
      const t = i / 360;
      const val = Math.sin(t * Math.PI * 6) * Math.exp(-t * 2.2) * 0.72;
      state.waveform.push(val);
    }
  }

  const loopBtn = document.getElementById('btn-loop-toggle');
  if (loopBtn) {
    loopBtn.addEventListener('click', () => {
      state.region.loop_enabled = !state.region.loop_enabled;
      sendFastRegion();
    });
  }

  const granBtn = document.getElementById('btn-granular-mode');
  if (granBtn) {
    granBtn.addEventListener('click', () => {
      const nextGran = ((state.params.granular_mode || 0) + 1) % 3;
      updateGranularModeUI(nextGran);
      sendIpc(`gran,${nextGran}`);
      showParamHudTooltip(granBtn, 'GRANULAR ENGINE', GRANULAR_MODE_LABELS[nextGran]);
      setTimeout(hideParamHudTooltip, 950);
    });
  }

  const dragWavBtn = document.getElementById('btn-drag-wav');
  if (dragWavBtn) {
    const triggerQuickWavExport = async () => {
      sendIpc('drag_wav_export');
      try {
        const resp = await fetch('/api/export-wav');
        if (!resp.ok) return;
        const blob = await resp.blob();
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        const safeName = (state.sampleName || 'tepui_clip').toLowerCase().replace(/[^a-z0-9_-]+/g, '_');
        a.href = url;
        a.download = `${safeName}_oled_clip.wav`;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        setTimeout(() => URL.revokeObjectURL(url), 2000);
      } catch (_) {}
    };
    dragWavBtn.addEventListener('click', () => {
      triggerQuickWavExport();
    });
    dragWavBtn.addEventListener('dragstart', (e) => {
      sendIpc('drag_wav_export');
      const safeName = (state.sampleName || 'tepui_clip').toLowerCase().replace(/[^a-z0-9_-]+/g, '_');
      if (e.dataTransfer) {
        e.dataTransfer.effectAllowed = 'copy';
        const wavUrl = `${window.location.origin}/api/export-wav`;
        e.dataTransfer.setData('DownloadURL', `audio/wav:${safeName}.wav:${wavUrl}`);
        e.dataTransfer.setData('text/uri-list', wavUrl);
      }
    });
  }

  const wt3dBtn = document.getElementById('btn-wt3d-toggle');
  if (wt3dBtn) {
    wt3dBtn.addEventListener('click', () => {
      state.wt3dMode = !state.wt3dMode;
      wt3dBtn.classList.toggle('active', !!state.wt3dMode);
      wt3dBtn.innerText = state.wt3dMode ? '3D WT' : '2D WAV';
      if (state.wt3dMode && (state.normalized.wt_pos || 0) < 0.02 && (state.normalized.wt_warp || 0) < 0.02) {
        state.normalized.wt_pos = 0.35;
        state.normalized.wt_warp = 0.25;
        state.params.wt_pos = 0.35;
        state.params.wt_warp = 0.25;
        updateKnobUI('wt_pos', 0.35);
        updateKnobUI('wt_warp', 0.25);
      } else if (!state.wt3dMode) {
        state.normalized.wt_pos = 0.0;
        state.normalized.wt_warp = 0.0;
        state.params.wt_pos = 0.0;
        state.params.wt_warp = 0.0;
        updateKnobUI('wt_pos', 0.0);
        updateKnobUI('wt_warp', 0.0);
      }
      sendIpc(`wt3d,${state.wt3dMode ? 1 : 0}`);
    });
  }

  if (!Array.isArray(state.customWtCycle) || state.customWtCycle.length !== 64) {
    state.customWtCycle = Array.from({ length: 64 }, (_, i) =>
      Math.sin((i / 64) * Math.PI * 2) * 0.8 + Math.sin((i / 64) * Math.PI * 4) * 0.2
    );
  }
  const wtDrawBtn = document.getElementById('btn-wtdraw-toggle');
  const pushCustomWtToDsp = () => {
    const csv = state.customWtCycle.map((v) => v.toFixed(3)).join(',');
    sendIpc(`wtdraw:${csv}`);
    // Reflejar 3 ciclos de la onda dibujada en el oscilograma principal
    const multi = [];
    for (let i = 0; i < 192; i++) {
      multi.push(state.customWtCycle[i % 64]);
    }
    state.waveform = multi;
    const telemRootEl = document.getElementById('telemetry-root');
    if (telemRootEl) {
      telemRootEl.innerText = 'CUSTOM WT DRAW • PINTA EN EL OLED';
    }
  };
  if (wtDrawBtn) {
    wtDrawBtn.addEventListener('click', () => {
      state.wtDrawMode = !state.wtDrawMode;
      wtDrawBtn.classList.toggle('active', !!state.wtDrawMode);
      if (state.wtDrawMode) {
        pushCustomWtToDsp();
      } else {
        sendIpc('wtdraw:clear');
      }
    });
    wtDrawBtn.addEventListener('dblclick', () => {
      state.wtDrawMode = false;
      wtDrawBtn.classList.remove('active');
      sendIpc('wtdraw:clear');
    });
  }

  const wavToWtBtn = document.getElementById('btn-wav-to-wt');
  if (wavToWtBtn) {
    wavToWtBtn.addEventListener('click', async () => {
      sendIpc('wtdraw:from_sample');
      state.wtDrawMode = true;
      if (wtDrawBtn) wtDrawBtn.classList.add('active');
      wavToWtBtn.classList.add('active');
      setTimeout(() => wavToWtBtn.classList.remove('active'), 800);

      try {
        const res = await fetch('tepui://localhost/api/wav-to-wt');
        if (res.ok) {
          const data = await res.json();
          if (Array.isArray(data.cycle) && data.cycle.length === 64) {
            state.customWtCycle = data.cycle;
            const multi = [];
            for (let i = 0; i < 192; i++) {
              multi.push(state.customWtCycle[i % 64]);
            }
            state.waveform = multi;
          }
        }
      } catch (_) {
        // Fallback local extraction from state.waveform if running in preview
        if (Array.isArray(state.waveform) && state.waveform.length >= 64) {
          for (let i = 0; i < 64; i++) {
            state.customWtCycle[i] = state.waveform[i % state.waveform.length] || 0;
          }
        }
      }
      const telemRootEl = document.getElementById('telemetry-root');
      if (telemRootEl) {
        telemRootEl.innerText = 'WAV → WT RESYNTHESIS • 64 PTS YIN';
      }
    });
  }

  const uniBtn = document.getElementById('btn-unison-voices');
  if (uniBtn) {
    const UNISON_STEPS = [1, 2, 4, 7, 8];
    uniBtn.addEventListener('click', () => {
      const cur = state.params.unison_voices || 1;
      const idx = UNISON_STEPS.indexOf(cur);
      const next = UNISON_STEPS[(idx + 1) % UNISON_STEPS.length];
      updateUnisonVoicesUI(next);
      sendIpc(`uni,${next}`);
    });
  }

  const fmBBtn = document.getElementById('btn-fm-b') || document.getElementById('btn-fm-b-toggle');
  if (fmBBtn) {
    const FM_STEPS = [0.0, 0.25, 0.55, 0.85];
    fmBBtn.addEventListener('click', () => {
      const cur = state.params.fm_from_b || 0.0;
      let next = FM_STEPS[1];
      for (let i = 0; i < FM_STEPS.length; i++) {
        if (Math.abs(cur - FM_STEPS[i]) < 0.08) {
          next = FM_STEPS[(i + 1) % FM_STEPS.length];
          break;
        }
      }
      updateFmFromBUI(next);
      sendIpc(`fmb,${next.toFixed(2)}`);
    });
  }

  const canvas = document.getElementById('oscilloscope');
  if (canvas) {
    const pickMarker = (normX, normY) => {
      const r = state.region;
      const candidates = [
        { id: 'trim_start', pos: r.trim_start_norm, preferTop: true },
        { id: 'trim_end', pos: r.trim_end_norm, preferTop: true },
      ];
      if (r.loop_enabled) {
        candidates.push({ id: 'loop_start', pos: r.loop_start_norm, preferTop: false });
        candidates.push({ id: 'loop_end', pos: r.loop_end_norm, preferTop: false });
      }
      let best = null;
      let bestScore = 0.055;
      for (const c of candidates) {
        let dist = Math.abs(normX - c.pos);
        if ((c.preferTop && normY < 0.5) || (!c.preferTop && normY >= 0.5)) {
          dist *= 0.65;
        }
        if (dist < bestScore) {
          bestScore = dist;
          best = c.id;
        }
      }
      return best;
    };

    let isDrawingWt = false;
    let isScrubbing3dWt = false;
    let lastDrawIdx = -1;
    let lastDrawVal = 0;

    const scrub3dWtAt = (normX, normY) => {
      const posVal = Math.max(0.0, Math.min(1.0, normX));
      const warpVal = Math.max(0.0, Math.min(1.0, 1.0 - normY));
      state.normalized.wt_pos = posVal;
      state.normalized.wt_warp = warpVal;
      state.params.wt_pos = posVal;
      state.params.wt_warp = warpVal;
      updateKnobUI('wt_pos', posVal);
      updateKnobUI('wt_warp', warpVal);
      sendFastParam('wt_pos', posVal);
      sendFastParam('wt_warp', warpVal);
      showParamHudTooltip(
        canvas,
        '3D TEPUI WT',
        `POS ${Math.round(posVal * 100)}% • WARP ${Math.round(warpVal * 100)}%`
      );
    };

    const paintCustomWtAt = (normX, normY) => {
      const idx = Math.max(0, Math.min(63, Math.floor(normX * 64)));
      const val = Math.max(-1.0, Math.min(1.0, (0.5 - normY) * 2.15));
      if (lastDrawIdx >= 0 && lastDrawIdx !== idx) {
        const step = idx > lastDrawIdx ? 1 : -1;
        const span = Math.abs(idx - lastDrawIdx);
        for (let k = 0; k <= span; k++) {
          const i = lastDrawIdx + k * step;
          const t = k / span;
          state.customWtCycle[i] = lastDrawVal * (1 - t) + val * t;
        }
      } else {
        state.customWtCycle[idx] = val;
      }
      lastDrawIdx = idx;
      lastDrawVal = val;
      pushCustomWtToDsp();
    };

    canvas.addEventListener('mousedown', (e) => {
      const rect = canvas.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;
      const normX = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
      const normY = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));
      if (state.wtDrawMode) {
        isDrawingWt = true;
        lastDrawIdx = -1;
        paintCustomWtAt(normX, normY);
        e.preventDefault();
        return;
      }
      if (state.wt3dMode) {
        isScrubbing3dWt = true;
        state.isDraggingKnob = true;
        scrub3dWtAt(normX, normY);
        e.preventDefault();
        return;
      }
      const picked = pickMarker(normX, normY);
      if (picked) {
        activeRegionMarker = picked;
        state.isDraggingRegion = true;
        document.body.style.cursor = 'ew-resize';
        e.preventDefault();
      }
    });

    window.addEventListener('mousemove', (e) => {
      const rect = canvas.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;
      const normX = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
      const normY = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));

      if (isDrawingWt && state.wtDrawMode) {
        paintCustomWtAt(normX, normY);
        return;
      }

      if (isScrubbing3dWt && state.wt3dMode && !state.wtDrawMode) {
        scrub3dWtAt(normX, normY);
        return;
      }

      if (!activeRegionMarker) {
        if (e.target === canvas) {
          if (state.wtDrawMode || state.wt3dMode) {
            canvas.style.cursor = 'crosshair';
          } else {
            const hover = pickMarker(normX, normY);
            canvas.style.cursor = hover ? 'ew-resize' : 'crosshair';
          }
        }
        return;
      }

      const r = state.region;
      if (activeRegionMarker === 'trim_start') {
        r.trim_start_norm = Math.max(0.0, Math.min(r.trim_end_norm - 0.02, normX));
        if (r.loop_start_norm < r.trim_start_norm) r.loop_start_norm = r.trim_start_norm;
      } else if (activeRegionMarker === 'trim_end') {
        r.trim_end_norm = Math.min(1.0, Math.max(r.trim_start_norm + 0.02, normX));
        if (r.loop_end_norm > r.trim_end_norm) r.loop_end_norm = r.trim_end_norm;
      } else if (activeRegionMarker === 'loop_start') {
        r.loop_start_norm = Math.max(r.trim_start_norm, Math.min(r.loop_end_norm - 0.02, normX));
      } else if (activeRegionMarker === 'loop_end') {
        r.loop_end_norm = Math.min(r.trim_end_norm, Math.max(r.loop_start_norm + 0.02, normX));
      }
      sendFastRegion();
    });

    window.addEventListener('mouseup', () => {
      if (isDrawingWt) {
        isDrawingWt = false;
        lastDrawIdx = -1;
      }
      if (isScrubbing3dWt) {
        isScrubbing3dWt = false;
        state.isDraggingKnob = false;
        hideParamHudTooltip();
      }
      if (activeRegionMarker) {
        activeRegionMarker = null;
        state.isDraggingRegion = false;
        document.body.style.cursor = 'default';
      }
    });

    canvas.addEventListener('dblclick', () => {
      state.region.trim_start_norm = 0.0;
      state.region.trim_end_norm = 1.0;
      state.region.loop_start_norm = 0.20;
      state.region.loop_end_norm = 0.85;
      sendFastRegion();
    });
  }

  updateRegionDisplay();
  startVisualizationLoop();
}

let layoutResizeGeneration = 1;
window.addEventListener('resize', () => {
  layoutResizeGeneration++;
});

const oledDirtyCache = {
  adsrSig: '',
  arpSig: '',
  frameTick: 0,
};

function startVisualizationLoop() {
  if (animFrameId) cancelAnimationFrame(animFrameId);

  const render = () => {
    oledDirtyCache.frameTick = (oledDirtyCache.frameTick + 1) | 0;
    const isPlaying =
      state.activeNotes.size > 0 ||
      (state.dspActiveVoices || 0) > 0 ||
      (state.vuLevelL || 0) > 0.005 ||
      (state.vuLevelR || 0) > 0.005;

    // 1. Osciloscopio 3D / Sampler y Filtro SVF + FFT
    drawOscilloscope();
    if (isPlaying || state.svfFftEnabled !== false || state.isDraggingSvfOled || (oledDirtyCache.frameTick & 1) === 0) {
      drawSvfOled();
    }

    // 2. Dirty-Flag para #oled-adsr: 0 redibujos redundantes en reposo si los faders no cambian
    const adsrSig = `${currentUiSkin}|${state.oledTheme}|${layoutResizeGeneration}|${(state.normalized.attack || 0).toFixed(4)}|${(state.normalized.decay || 0).toFixed(4)}|${(state.normalized.sustain || 0).toFixed(4)}|${(state.normalized.release || 0).toFixed(4)}|${state.params.env2_target || 0}|${(state.params.env2_depth || 0).toFixed(3)}|${(state.params.env_curve || 0).toFixed(3)}|${state.activeAdsrDragNode || ''}`;
    if (isPlaying || state.activeAdsrDragNode || adsrSig !== oledDirtyCache.adsrSig) {
      oledDirtyCache.adsrSig = adsrSig;
      drawAdsrOled();
    }

    // 3. LFO y Arpegiador con planificación adaptativa
    drawLfoOled();
    const arpMode = state.params.arp_mode || 0;
    const arpBars = getArpBarsForMode(arpMode);
    const arpSig = `${currentUiSkin}|${state.oledTheme}|${layoutResizeGeneration}|${arpMode}|${state.params.arp_step_count || 12}|${state.params.arp_rate_div || 2}|${state.lastEditedArpStep}|${arpBars.join(',')}`;
    if (isPlaying || arpMode > 0 || arpSig !== oledDirtyCache.arpSig || (oledDirtyCache.frameTick % 4) === 0) {
      oledDirtyCache.arpSig = arpSig;
      drawArpOled();
    }

    // 4. FX Rack, Medidores VU y Anillos de Modulación
    drawReverbOled();
    updateVuMeters();
    updateLiveKnobModulationRings();
    animPhase += 0.05;
    animFrameId = requestAnimationFrame(render);
  };

  animFrameId = requestAnimationFrame(render);
}

const UI_SKINS = [
  { id: 'minimal', className: 'skin-minimal', btnId: 'btn-skin-minimal', label: 'MINIMAL', fullLabel: 'SWISS DAYLIGHT LAB' },
  { id: 'dark', className: 'skin-dark', btnId: 'btn-skin-dark', label: 'DARK', fullLabel: 'TEPUI DARK STUDIO' },
  { id: 'retro', className: 'skin-retro', btnId: 'btn-skin-retro', label: 'RETRO', fullLabel: 'CREAM CONSOLE' },
  { id: 'synth70', className: 'skin-synth70', btnId: 'btn-skin-synth70', label: 'SYNTH70', fullLabel: "SYNTH '70S" },
  { id: 'gunmetal', className: 'skin-gunmetal', btnId: 'btn-skin-gunmetal', label: 'GUNMETAL', fullLabel: 'GUNMETAL RACK' },
  { id: 'phosphor', className: 'skin-phosphor', btnId: 'btn-skin-phosphor', label: 'PHOSPHOR', fullLabel: 'CRT PHOSPHOR' },
];

let currentUiSkin = 'minimal';

function getCanvasSkinPalette() {
  if (currentUiSkin === 'minimal') {
    return {
      isDaylight: true,
      bgTop: '#efede6',
      bgBot: '#e6e3db',
      rulerBg: 'rgba(230, 227, 218, 0.96)',
      gridLine: 'rgba(23, 24, 26, 0.09)',
      zeroLine: 'rgba(224, 60, 31, 0.25)',
      primaryHex: '#e03c1f',
      primaryRgba: (a) => `rgba(224, 60, 31, ${a})`,
      secondaryHex: '#17181a',
      secondaryRgba: (a) => `rgba(23, 24, 26, ${a})`,
      amberHex: '#d97706',
      amberRgba: (a) => `rgba(217, 119, 6, ${a})`,
      textPrimary: '#17181a',
      textMuted: 'rgba(23, 24, 26, 0.72)',
      nodeFill: '#faf9f5',
    };
  }
  if (currentUiSkin === 'retro') {
    return {
      isDaylight: true,
      bgTop: '#e4decb',
      bgBot: '#dad2bc',
      rulerBg: 'rgba(214, 205, 184, 0.96)',
      gridLine: 'rgba(35, 31, 28, 0.10)',
      zeroLine: 'rgba(200, 75, 49, 0.26)',
      primaryHex: '#c84b31',
      primaryRgba: (a) => `rgba(200, 75, 49, ${a})`,
      secondaryHex: '#231f1c',
      secondaryRgba: (a) => `rgba(35, 31, 28, ${a})`,
      amberHex: '#b45309',
      amberRgba: (a) => `rgba(180, 83, 9, ${a})`,
      textPrimary: '#231f1c',
      textMuted: 'rgba(35, 31, 28, 0.72)',
      nodeFill: '#f7f2e7',
    };
  }
  if (currentUiSkin === 'synth70') {
    return {
      isDaylight: false,
      bgTop: '#161311',
      bgBot: '#0f0d0b',
      rulerBg: 'rgba(18, 15, 13, 0.94)',
      gridLine: 'rgba(244, 232, 193, 0.11)',
      zeroLine: 'rgba(224, 109, 83, 0.28)',
      primaryHex: '#e06d53',
      primaryRgba: (a) => `rgba(224, 109, 83, ${a})`,
      secondaryHex: '#eab308',
      secondaryRgba: (a) => `rgba(234, 179, 8, ${a})`,
      amberHex: '#eab308',
      amberRgba: (a) => `rgba(234, 179, 8, ${a})`,
      textPrimary: '#f4e8c1',
      textMuted: 'rgba(244, 232, 193, 0.72)',
      nodeFill: '#f4e8c1',
    };
  }
  if (currentUiSkin === 'gunmetal') {
    return {
      isDaylight: false,
      bgTop: '#141820',
      bgBot: '#0d1016',
      rulerBg: 'rgba(14, 17, 22, 0.94)',
      gridLine: 'rgba(148, 163, 184, 0.14)',
      zeroLine: 'rgba(245, 158, 11, 0.28)',
      primaryHex: '#f59e0b',
      primaryRgba: (a) => `rgba(245, 158, 11, ${a})`,
      secondaryHex: '#38bdf8',
      secondaryRgba: (a) => `rgba(56, 189, 248, ${a})`,
      amberHex: '#f59e0b',
      amberRgba: (a) => `rgba(245, 158, 11, ${a})`,
      textPrimary: '#e5e9f0',
      textMuted: 'rgba(203, 213, 225, 0.76)',
      nodeFill: '#ffffff',
    };
  }
  if (currentUiSkin === 'phosphor') {
    return {
      isDaylight: false,
      bgTop: '#040a05',
      bgBot: '#020503',
      rulerBg: 'rgba(3, 8, 4, 0.94)',
      gridLine: 'rgba(57, 255, 136, 0.14)',
      zeroLine: 'rgba(57, 255, 136, 0.28)',
      primaryHex: '#39ff88',
      primaryRgba: (a) => `rgba(57, 255, 136, ${a})`,
      secondaryHex: '#16a34a',
      secondaryRgba: (a) => `rgba(22, 163, 74, ${a})`,
      amberHex: '#39ff88',
      amberRgba: (a) => `rgba(57, 255, 136, ${a})`,
      textPrimary: '#d1fae5',
      textMuted: 'rgba(110, 231, 183, 0.78)',
      nodeFill: '#39ff88',
    };
  }
  return {
    isDaylight: false,
    bgTop: '#03060a',
    bgBot: '#03060a',
    rulerBg: 'rgba(4, 7, 13, 0.92)',
    gridLine: 'rgba(30, 45, 68, 0.45)',
    zeroLine: 'rgba(56, 189, 248, 0.24)',
    primaryHex: '#00ebff',
    primaryRgba: (a) => `rgba(0, 235, 255, ${a})`,
    secondaryHex: '#ffaa00',
    secondaryRgba: (a) => `rgba(255, 170, 0, ${a})`,
    amberHex: '#ffaa00',
    amberRgba: (a) => `rgba(255, 170, 0, ${a})`,
    textPrimary: '#e2e8f0',
    textMuted: 'rgba(186, 204, 228, 0.86)',
    nodeFill: '#ffffff',
  };
}

function applyUiSkin(skinId, persist = true) {
  hideParamHudTooltip();
  const found = UI_SKINS.find((s) => s.id === skinId) || UI_SKINS[0];
  currentUiSkin = found.id;
  state.uiSkin = found.id;
  synthStore.setUiSkin(found.id);

  if (document.body) {
    UI_SKINS.forEach((s) => document.body.classList.remove(s.className));
    document.body.classList.add(found.className);
    document.body.dataset.uiSkin = found.id;
  }

  UI_SKINS.forEach((s) => {
    const cardBtn = document.getElementById(s.btnId);
    if (cardBtn) {
      const isAct = s.id === found.id;
      cardBtn.classList.toggle('active', isAct);
      cardBtn.setAttribute('aria-pressed', isAct ? 'true' : 'false');
    }
  });

  const activePill = document.getElementById('active-skin-pill');
  if (activePill) {
    activePill.innerText = `ACTIVO: ${found.fullLabel}`;
  }

  const btnTheme = document.getElementById('btn-oled-theme');
  if (btnTheme) {
    btnTheme.innerText = `SKIN: ${found.label}`;
  }

  if (persist) {
    try {
      localStorage.setItem('tepui_ui_skin', found.id);
    } catch (_e) {}
  }

  layoutResizeGeneration++;
  oledDirtyCache.adsrSig = '';
  oledDirtyCache.arpSig = '';

  const bgStrokeMap = {
    minimal: 'rgba(23, 24, 26, 0.18)',
    retro: 'rgba(35, 31, 28, 0.20)',
    synth70: 'rgba(244, 232, 193, 0.16)',
    gunmetal: 'rgba(148, 163, 184, 0.18)',
    phosphor: 'rgba(57, 255, 136, 0.18)',
    dark: '#161e2b',
  };
  const bgTrackStroke = bgStrokeMap[currentUiSkin] || '#161e2b';
  document.querySelectorAll('.knob-container .knob-bg-track').forEach((circle) => {
    circle.setAttribute('stroke', bgTrackStroke);
  });

  document.querySelectorAll('.knob-container').forEach((el) => {
    const paramId = el.dataset.param;
    if (paramId) {
      updateKnobUI(
        paramId,
        state.normalized && state.normalized[paramId] !== undefined
          ? state.normalized[paramId]
          : 0.5
      );
    }
  });

  drawSvfOled();
  drawAdsrOled();
  drawLfoOled();
  drawArpOled();
  drawReverbOled();
  drawOscilloscope();
}

function sizeOledCanvas(canvas) {
  const dpr = window.devicePixelRatio || 1;
  if (canvas._cachedResizeGen === layoutResizeGeneration && canvas._cachedDpr === dpr && canvas.width > 0 && canvas.height > 0) {
    return dpr;
  }
  const rect = canvas.getBoundingClientRect();
  const targetW = Math.round(rect.width * dpr);
  const targetH = Math.round(rect.height * dpr);
  if (targetW > 0 && targetH > 0) {
    if (canvas.width !== targetW || canvas.height !== targetH) {
      canvas.width = targetW;
      canvas.height = targetH;
    }
    canvas._cachedResizeGen = layoutResizeGeneration;
    canvas._cachedDpr = dpr;
  }
  return dpr;
}

function getOledPrimaryColor() {
  return getCanvasSkinPalette().primaryHex;
}

function getOledPrimaryRgba(alpha) {
  return getCanvasSkinPalette().primaryRgba(alpha);
}

function getOledSecondaryColor() {
  return getCanvasSkinPalette().secondaryHex;
}

function getOledSecondaryRgba(alpha) {
  return getCanvasSkinPalette().secondaryRgba(alpha);
}

function drawSvfOled() {
  const canvas = document.getElementById('oled-svf');
  if (!canvas) return;
  const dpr = sizeOledCanvas(canvas);
  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;
  if (w <= 0 || h <= 0) return;

  const pal = getCanvasSkinPalette();
  const primaryHex = pal.primaryHex;
  const secondaryHex = pal.secondaryHex;
  const plotW = w - 16 * dpr; // Reserva margen derecho para escala dB y medidor vertical

  const bgGrad = ctx.createLinearGradient(0, 0, 0, h);
  bgGrad.addColorStop(0, pal.bgTop);
  bgGrad.addColorStop(1, pal.bgBot);
  ctx.fillStyle = bgGrad;
  ctx.fillRect(0, 0, w, h);

  // 1. Rejilla logarítmica completa (20Hz .. 20kHz) y divisiones decibélicas (+20 .. -30 dB)
  ctx.strokeStyle = pal.gridLine;
  ctx.lineWidth = 1;
  const freqTicks = [
    { norm: 0.04, lbl: '20Hz' },
    { norm: 0.14, lbl: '50' },
    { norm: 0.24, lbl: '100' },
    { norm: 0.34, lbl: '200' },
    { norm: 0.47, lbl: '500' },
    { norm: 0.58, lbl: '1k' },
    { norm: 0.69, lbl: '2k' },
    { norm: 0.81, lbl: '5k' },
    { norm: 0.90, lbl: '10k' },
    { norm: 0.98, lbl: '20k' }
  ];
  const rulerH = 12 * dpr;
  const botPlotY = h - rulerH;

  freqTicks.forEach((ft) => {
    const x = Math.round(ft.norm * plotW);
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, botPlotY);
    ctx.stroke();
  });

  const dbRows = [
    { yNorm: 0.16, lbl: '+20' },
    { yNorm: 0.34, lbl: '+10' },
    { yNorm: 0.52, lbl: '0' },
    { yNorm: 0.70, lbl: '-15' },
    { yNorm: 0.86, lbl: '-30' }
  ];
  dbRows.forEach((row) => {
    const y = Math.round(row.yNorm * botPlotY);
    ctx.strokeStyle = row.lbl === '0' ? pal.zeroLine : pal.gridLine;
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(plotW, y);
    ctx.stroke();
    ctx.fillStyle = pal.textMuted;
    ctx.font = `${Math.round(5.8 * dpr)}px monospace`;
    ctx.fillText(row.lbl, plotW + 2 * dpr, y + 2 * dpr);
  });

  const cutNorm = state.normalized.cutoff !== undefined ? state.normalized.cutoff : 1.0;
  const resNorm = state.normalized.resonance !== undefined ? state.normalized.resonance : 0.06;
  const driveNorm = state.normalized.drive !== undefined ? state.normalized.drive : 0.22;
  const ottNorm = state.normalized.ott !== undefined ? state.normalized.ott : 0.30;
  const wtPosNorm = state.normalized.wt_pos !== undefined ? state.normalized.wt_pos : 0.35;
  const mode = state.params.filter_mode || 0;
  const lfoDepth = (state.params.lfo_target === 0) ? (state.params.lfo_depth || 0) : 0;
  const modOffset = lfoDepth * Math.sin(animPhase) * 0.16;
  const effCut = Math.max(0.06, Math.min(0.95, cutNorm + modOffset));
  const peakBoost = resNorm * 0.44;

  const evalCurve = (xNorm, cPos, m, extraQ = 1.0) => {
    const d = (xNorm - cPos) * 7.5;
    const resPeak = peakBoost * extraQ * Math.exp(-(d * d) * 3.8);
    if (m === 3) return 0.50; // Bypass
    if (m === 4) { // Moog Ladder 24dB/oct 4-pole non-linear saturation
      const base4 = 1.0 / (1.0 + Math.exp(d * 4.4));
      const ladderRes = peakBoost * 1.38 * extraQ * Math.exp(-(d * d) * 5.2);
      return Math.min(0.97, base4 * 0.62 + ladderRes);
    }
    if (m === 5) { // Karplus-Strong Tuned Comb Filter (multi-tooth harmonic interference)
      const combPeriod = 6.0 + (1.0 - cPos) * 14.0;
      const teeth = 0.5 + 0.5 * Math.cos((xNorm - cPos) * Math.PI * combPeriod);
      const envDecay = 1.0 / (1.0 + Math.max(0, xNorm - cPos) * 2.4);
      return Math.min(0.96, (0.18 + teeth * (0.48 + resNorm * 0.34)) * envDecay);
    }
    if (m === 6) { // Vocal Formant Filter (A-E-I-O-U 3-peak resonator)
      const f1 = 0.18 + cPos * 0.22;
      const f2 = 0.42 + cPos * 0.30;
      const f3 = 0.72 + cPos * 0.18;
      const p1 = Math.exp(-Math.pow((xNorm - f1) * 10.5, 2)) * 0.74;
      const p2 = Math.exp(-Math.pow((xNorm - f2) * 9.5, 2)) * 0.64;
      const p3 = Math.exp(-Math.pow((xNorm - f3) * 11.0, 2)) * 0.50;
      return Math.min(0.96, 0.12 + (p1 + p2 + p3) * (0.75 + resNorm * 0.35));
    }
    if (m === 0) { // Lowpass
      const base = 1.0 / (1.0 + Math.exp(d * 2.5));
      return Math.min(0.96, base * 0.56 + resPeak);
    }
    if (m === 1) { // Highpass
      const base = 1.0 / (1.0 + Math.exp(-d * 2.5));
      return Math.min(0.96, base * 0.56 + resPeak);
    }
    // Bandpass
    return Math.min(0.96, Math.exp(-(d * d) * 1.15) * (0.56 + resPeak * 0.72));
  };

  // 2. ANALIZADOR ESPECTRAL FFT DE 64 BANDAS EN TIEMPO REAL (FabFilter Pro-Q 4 / Serum 2 Tier)
  if (!state.fftBins64) {
    state.fftBins64 = new Float32Array(64);
    state.fftPeaks64 = new Float32Array(64);
  }
  const isPlaying = state.activeNotes.size > 0 || (state.dspActiveVoices || 0) > 0 || (state.vuLevelL || 0) > 0.04;
  const liveWebBins = (typeof window.__tepuiGetFftBins === 'function') ? window.__tepuiGetFftBins() : null;
  const audioEnergy = isPlaying
    ? Math.max(0.28, Math.min(1.0, ((state.vuLevelL || 0.35) + (state.vuLevelR || 0.35)) * 0.75 + 0.25))
    : 0.26; // Piso armónico vivo en reposo para previsualización continua del filtro

  const maxPlotH = botPlotY - 8 * dpr;

  if (state.svfFftEnabled !== false) {
    const numBands = 64;
    const bandSlotW = plotW / numBands;
    const barW = Math.max(1.2 * dpr, bandSlotW * 0.64);
    const sharedBarGrad = ctx.createLinearGradient(0, 8 * dpr, 0, botPlotY);
    sharedBarGrad.addColorStop(0, getOledPrimaryRgba(pal.isDaylight ? 0.32 : (isPlaying ? 0.56 : 0.40)));
    sharedBarGrad.addColorStop(0.55, getOledPrimaryRgba(pal.isDaylight ? 0.14 : (isPlaying ? 0.28 : 0.18)));
    sharedBarGrad.addColorStop(1, getOledPrimaryRgba(0.02));
    const capColor = pal.isDaylight ? pal.secondaryHex : (isPlaying ? '#ffffff' : getOledPrimaryRgba(0.78));
    const capH = Math.max(1.2 * dpr, 1.5 * dpr);

    for (let b = 0; b < numBands; b++) {
      const bNorm = b / (numBands - 1);
      const tf = evalCurve(bNorm, effCut, mode, 1.15);
      let targetMag = 0;

      if (liveWebBins && liveWebBins.length > 0) {
        // Mapeo logarítmico desde los bins reales del AnalyserNode WebAudio
        const logIdx = Math.min(liveWebBins.length - 1, Math.floor(Math.pow(bNorm, 1.75) * (liveWebBins.length - 1)));
        targetMag = (liveWebBins[logIdx] / 255.0) * (0.45 + tf * 0.75);
      } else {
        // Síntesis espectral armónica sincronizada con el motor DSP (armónicos + formantes + filtro SVF)
        const harmComb = Math.abs(Math.sin(bNorm * Math.PI * (7.0 + wtPosNorm * 11.0) + animPhase * (isPlaying ? 1.8 : 0.45)));
        const subEnergy = Math.exp(-bNorm * 2.2) * 0.62;
        const formantPeak = Math.exp(-Math.pow((bNorm - (0.28 + wtPosNorm * 0.35)) * 4.5, 2)) * (0.42 + driveNorm * 0.35);
        const airShimmer = Math.sin(b * 1.9 + animPhase * 2.4) * 0.12 + ottNorm * 0.22;
        const rawSpec = (subEnergy + harmComb * 0.34 + formantPeak + airShimmer) * audioEnergy;
        targetMag = Math.max(0.05, Math.min(0.95, rawSpec * (0.18 + tf * 1.05)));
      }

      // Suavizado balístico de ataque rápido y caída exponencial
      const prev = state.fftBins64[b] || 0;
      const smoothed = targetMag > prev
        ? prev + (targetMag - prev) * 0.52
        : prev * 0.88 + targetMag * 0.12;
      state.fftBins64[b] = smoothed;

      if (smoothed >= (state.fftPeaks64[b] || 0)) {
        state.fftPeaks64[b] = smoothed;
      } else {
        state.fftPeaks64[b] = Math.max(0, (state.fftPeaks64[b] || 0) - 0.008);
      }

      const bx = b * bandSlotW + (bandSlotW - barW) * 0.5;
      const bh = smoothed * maxPlotH * 0.86;
      const by = botPlotY - bh;

      // Columna espectral de la banda b
      ctx.fillStyle = sharedBarGrad;
      ctx.fillRect(bx, by, barW, bh);

      // Peak-hold cap flotante por banda
      const peakY = botPlotY - state.fftPeaks64[b] * maxPlotH * 0.86;
      ctx.fillStyle = capColor;
      ctx.fillRect(bx, Math.max(6 * dpr, peakY), barW, capH);
    }

    // Trazo continuo de alta resolución FFT (estilo FabFilter Pro-Q 4)
    ctx.beginPath();
    for (let b = 0; b < numBands; b++) {
      const bx = (b + 0.5) * bandSlotW;
      const jitter = Math.sin(b * 3.7 + animPhase * 2.8) * 0.025 * audioEnergy;
      const by = botPlotY - Math.max(0.03, state.fftBins64[b] + jitter) * maxPlotH * 0.88;
      if (b === 0) ctx.moveTo(bx, by);
      else ctx.lineTo(bx, by);
    }
    ctx.strokeStyle = getOledPrimaryRgba(0.85);
    ctx.lineWidth = 1.15 * dpr;
    ctx.stroke();
    svfSpectrumSource.set(state.fftBins64.map((v) => -18 + Math.max(0, Math.min(1, v)) * 32));
  }

  // 3. Curva secundaria armónica / de modulación bipolar (Ámbar / Complementaria, estilo Propuesta A)
  const companionCut = cutNorm > 0.88
    ? 0.56
    : Math.max(0.14, Math.min(0.88, cutNorm - 0.14 + (driveNorm * 0.22)));
  ctx.beginPath();
  for (let px = 0; px <= plotW; px += 2) {
    const xn = px / plotW;
    const dComp = (xn - companionCut) * 6.5;
    const yn = Math.min(0.92, Math.exp(-(dComp * dComp) * 1.15) * (0.62 + resNorm * 0.26));
    const y = botPlotY - yn * maxPlotH;
    if (px === 0) ctx.moveTo(px, y);
    else ctx.lineTo(px, y);
  }
  ctx.strokeStyle = getOledSecondaryRgba(0.78);
  ctx.lineWidth = 1.35 * dpr;
  ctx.stroke();

  // 4. La curva principal SVF y su nodo interactivo 2D son renderizados por <Plot.Root> (@audio-ui/react/plot)
  const nodeX = Math.max(8 * dpr, Math.min(plotW - 8 * dpr, effCut * plotW));
  const nodeCurveY = botPlotY - evalCurve(effCut, effCut, mode) * maxPlotH;
  const nodeY = Math.max(8 * dpr, Math.min(botPlotY - 6 * dpr, nodeCurveY));

  // Etiqueta flotante sobre el nodo activo (Freq Hz + Q)
  const calloutX = Math.max(6 * dpr, Math.min(plotW - 64 * dpr, nodeX - 24 * dpr));
  const calloutY = Math.max(18 * dpr, nodeY - 8 * dpr);
  ctx.fillStyle = pal.textPrimary;
  ctx.font = `bold ${Math.round(6.6 * dpr)}px monospace`;
  ctx.fillText(`${formatKnobValue('cutoff', cutNorm)}`, calloutX, calloutY);

  // 6. Mini Medidor Vertical Post-Filtro en el borde derecho del OLED (Propuesta A)
  const meterX = w - 4.5 * dpr;
  const meterH = botPlotY - 8 * dpr;
  ctx.fillStyle = pal.isDaylight ? 'rgba(23, 24, 26, 0.14)' : 'rgba(15, 23, 42, 0.9)';
  ctx.fillRect(meterX, 6 * dpr, 3 * dpr, meterH);
  const outLevel = Math.min(1.0, audioEnergy * (0.45 + evalCurve(effCut, effCut, mode) * 0.55));
  const fillH = outLevel * meterH;
  const mGrad = ctx.createLinearGradient(0, botPlotY - fillH, 0, botPlotY);
  mGrad.addColorStop(0, secondaryHex);
  mGrad.addColorStop(0.35, primaryHex);
  mGrad.addColorStop(1, getOledPrimaryRgba(0.35));
  ctx.fillStyle = mGrad;
  ctx.fillRect(meterX, botPlotY - fillH, 3 * dpr, fillH);

  // 7. Regla de Frecuencias Logarítmica dedicada en la base (sin pisar las barras FFT)
  ctx.fillStyle = pal.rulerBg;
  ctx.fillRect(0, botPlotY, w, rulerH);
  ctx.strokeStyle = pal.gridLine;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, botPlotY);
  ctx.lineTo(w, botPlotY);
  ctx.stroke();

  ctx.fillStyle = pal.textMuted;
  ctx.font = `bold ${Math.round(5.8 * dpr)}px monospace`;
  freqTicks.forEach((ft, idx) => {
    if (idx === 1 || idx === 3 || idx === 6) return;
    const tx = Math.max(3 * dpr, Math.min(plotW - 18 * dpr, Math.round(ft.norm * plotW - 6 * dpr)));
    ctx.fillText(ft.lbl, tx, h - 2.8 * dpr);
  });

  // HUD telemetría superior izquierda
  const modeNames = ['LP 24dB', 'HP 24dB', 'BP 12dB', 'BYPASS', 'MOOG LADDER 24', 'COMB K-S', 'FORMANT A-E-I'];
  ctx.fillStyle = primaryHex;
  ctx.font = `bold ${Math.round(7.2 * dpr)}px monospace`;
  ctx.fillText(
    `${modeNames[mode] || 'LP'} • ${formatKnobValue('cutoff', cutNorm)} • Q:${formatKnobValue('resonance', resNorm)}`,
    6 * dpr,
    10 * dpr
  );
}

function drawAdsrOled() {
  const canvas = document.getElementById('oled-adsr');
  if (!canvas) return;
  const dpr = sizeOledCanvas(canvas);
  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;
  if (w <= 0 || h <= 0) return;

  const pal = getCanvasSkinPalette();
  const envHex = pal.isDaylight ? pal.primaryHex : (currentUiSkin === 'dark' ? (state.oledTheme === 'emerald' ? '#10b981' : '#ffaa00') : pal.primaryHex);
  const envRgba = (a) => pal.isDaylight ? pal.primaryRgba(a) : (currentUiSkin === 'dark' ? (state.oledTheme === 'emerald' ? `rgba(16, 185, 129, ${a})` : `rgba(255, 170, 0, ${a})`) : pal.primaryRgba(a));
  const accHex = pal.isDaylight ? pal.secondaryHex : (currentUiSkin === 'dark' ? (state.oledTheme === 'emerald' ? '#00ebff' : '#ffd166') : pal.secondaryHex);

  const bgGrad = ctx.createLinearGradient(0, 0, 0, h);
  bgGrad.addColorStop(0, pal.isDaylight ? pal.bgTop : (currentUiSkin === 'dark' ? '#060503' : pal.bgTop));
  bgGrad.addColorStop(1, pal.isDaylight ? pal.bgBot : (currentUiSkin === 'dark' ? '#060503' : pal.bgBot));
  ctx.fillStyle = bgGrad;
  ctx.fillRect(0, 0, w, h);

  const aNorm = state.normalized.attack !== undefined ? state.normalized.attack : 0.02;
  const dNorm = state.normalized.decay !== undefined ? state.normalized.decay : 0.15;
  const sNorm = state.normalized.sustain !== undefined ? state.normalized.sustain : 0.8;
  const rNorm = state.normalized.release !== undefined ? state.normalized.release : 0.15;
  const curveTension = state.params.env_curve !== undefined ? state.params.env_curve : -0.65;

  // Coordenadas normalizadas sincronizadas con <Envelope.Root> (@audio-ui/react/envelope)
  const padX = 10 * dpr;
  const rulerH = 12 * dpr;
  const topY = 16 * dpr;
  const botY = h - rulerH - 3 * dpr;
  const spanW = w - padX * 2;

  const x1Norm = 0.02 + aNorm * 0.22;
  const x2Norm = x1Norm + 0.02 + dNorm * 0.22;
  const x3Norm = x2Norm + 0.18;
  const x4Norm = x3Norm + 0.02 + rNorm * 0.22;

  const x0 = padX;
  const x1 = x0 + x1Norm * spanW;
  const x2 = x0 + x2Norm * spanW;
  const x3 = x0 + x3Norm * spanW;
  const x4 = x0 + x4Norm * spanW;

  const s = Math.max(0.06, Math.min(0.96, sNorm));
  const ySus = botY - s * (botY - topY);

  // Guardar coordenadas normalizadas de los nodos para arrastre directo 2D
  state.adsrNodesLayout = {
    x0: x0 / w,
    x1: x1 / w,
    x2: x2 / w,
    x3: x3 / w,
    x4: x4 / w,
    topY: topY / h,
    botY: botY / h,
    ySus: ySus / h
  };

  // 1. Grilla horizontal de amplitud (100%, 75%, 50%, 25%) y columnas de etapa (A | D | S | R)
  ctx.lineWidth = 1;
  [0.25, 0.5, 0.75].forEach((lvl) => {
    const gy = topY + lvl * (botY - topY);
    ctx.strokeStyle = pal.gridLine;
    ctx.beginPath(); ctx.moveTo(0, gy); ctx.lineTo(w, gy); ctx.stroke();
  });

  const stageBounds = [
    { xStart: x0, xEnd: x1, lbl: 'ATTACK', val: formatKnobValue('attack', aNorm) },
    { xStart: x1, xEnd: x2, lbl: 'DECAY', val: formatKnobValue('decay', dNorm) },
    { xStart: x2, xEnd: x3, lbl: 'SUSTAIN', val: `${Math.round(sNorm * 100)}%` },
    { xStart: x3, xEnd: x4, lbl: 'RELEASE', val: formatKnobValue('release', rNorm) }
  ];

  stageBounds.forEach((st, idx) => {
    if (idx % 2 === 1) {
      ctx.fillStyle = pal.isDaylight ? 'rgba(23, 24, 26, 0.03)' : 'rgba(255, 170, 0, 0.025)';
      ctx.fillRect(st.xStart, topY - 4 * dpr, st.xEnd - st.xStart, botY - topY + 4 * dpr);
    }
    ctx.save();
    ctx.strokeStyle = pal.isDaylight ? pal.secondaryRgba(0.18) : envRgba(0.18);
    ctx.setLineDash([2.5 * dpr, 2.5 * dpr]);
    ctx.beginPath();
    ctx.moveTo(st.xEnd, topY - 3 * dpr);
    ctx.lineTo(st.xEnd, botY);
    ctx.stroke();
    ctx.restore();

    const colW = st.xEnd - st.xStart;
    if (colW > 28 * dpr) {
      ctx.fillStyle = pal.isDaylight ? pal.secondaryRgba(0.55) : envRgba(0.45);
      ctx.font = `bold ${Math.round(5.4 * dpr)}px monospace`;
      ctx.fillText(st.lbl[0], st.xStart + 3 * dpr, topY + 7 * dpr);
    }
  });

  const bowBias = curveTension * 0.28;
  const c1y = topY + (botY - topY) * (0.42 - bowBias);
  const c2y = ySus - (ySus - topY) * (0.35 + bowBias);
  const c3y = botY - (botY - ySus) * (0.35 + bowBias);
  const evalQuad = (t, p0, pc, p1) => (1 - t) * (1 - t) * p0 + 2 * (1 - t) * t * pc + t * t * p1;

  // 2.5 Overlay de Segunda Envolvente Dedicada (MOD ENV 2) en cian punteado
  const env2Depth = state.params.env2_depth !== undefined ? state.params.env2_depth : 0.35;
  if (Math.abs(env2Depth) > 0.02) {
    const e2Top = topY + (1.0 - Math.min(1.0, Math.abs(env2Depth) * 0.92)) * (botY - topY) * 0.45;
    const e2Sus = botY - (s * 0.65) * (botY - topY);
    const e2x1 = x0 + (x1 - x0) * 0.68;
    const e2x2 = e2x1 + (x2 - x1) * 0.72;
    ctx.save();
    ctx.strokeStyle = currentUiSkin === 'dark' ? 'rgba(0, 235, 255, 0.82)' : pal.secondaryRgba(0.75);
    ctx.lineWidth = 1.35 * dpr;
    ctx.setLineDash([3 * dpr, 2.5 * dpr]);
    ctx.beginPath();
    ctx.moveTo(x0, botY);
    ctx.quadraticCurveTo(x0 + (e2x1 - x0) * 0.4, e2Top + (botY - e2Top) * 0.25, e2x1, e2Top);
    ctx.quadraticCurveTo(e2x1 + (e2x2 - e2x1) * 0.35, e2Sus, e2x2, e2Sus);
    ctx.lineTo(x3, e2Sus);
    ctx.quadraticCurveTo(x3 + (x4 - x3) * 0.35, botY, x4, botY);
    ctx.stroke();
    ctx.restore();
  }

  // 6. Cursor animado de envolvente en vivo cuando hay notas activas (la curva ADSR principal y sus nodos los dibuja <Envelope.Root>)
  const isPlaying = state.activeNotes.size > 0 || (state.dspActiveVoices || 0) > 0;
  if (isPlaying) {
    const envPhase = ((animPhase * 0.25) % 1.0 + 1.0) % 1.0;
    const playX = x0 + envPhase * spanW;
    let playY = botY;
    if (playX <= x1) {
      const t = (playX - x0) / Math.max(1, x1 - x0);
      playY = evalQuad(t, botY, c1y, topY);
    } else if (playX <= x2) {
      const t = (playX - x1) / Math.max(1, x2 - x1);
      playY = evalQuad(t, topY, c2y, ySus);
    } else if (playX <= x3) {
      playY = ySus;
    } else {
      const t = (playX - x3) / Math.max(1, x4 - x3);
      playY = evalQuad(t, ySus, c3y, botY);
    }
    ctx.save();
    ctx.strokeStyle = pal.isDaylight ? pal.secondaryRgba(0.35) : 'rgba(255, 255, 255, 0.35)';
    ctx.lineWidth = 1 * dpr;
    ctx.beginPath(); ctx.moveTo(playX, topY); ctx.lineTo(playX, botY); ctx.stroke();
    ctx.fillStyle = pal.isDaylight ? pal.primaryHex : '#ffffff';
    ctx.shadowColor = envHex;
    ctx.shadowBlur = pal.isDaylight ? 0 : 8 * dpr;
    ctx.beginPath(); ctx.arc(playX, playY, 3.2 * dpr, 0, Math.PI * 2); ctx.fill();
    ctx.restore();
  }

  // 7. Regla de Tiempo Auto-Zoom inferior
  ctx.fillStyle = pal.rulerBg;
  ctx.fillRect(0, h - rulerH, w, rulerH);
  ctx.strokeStyle = pal.gridLine;
  ctx.lineWidth = 1;
  ctx.beginPath(); ctx.moveTo(0, h - rulerH); ctx.lineTo(w, h - rulerH); ctx.stroke();

  const aSec = 0.001 * Math.pow(5.0 / 0.001, aNorm);
  const dSec = 0.001 * Math.pow(5.0 / 0.001, dNorm);
  const rSec = 0.001 * Math.pow(10.0 / 0.001, rNorm);
  const totSec = aSec + dSec + rSec;
  const fmtSec = (sec) => sec >= 1.0 ? `${sec.toFixed(2)}s` : `${Math.max(1, Math.round(sec * 1000))}ms`;

  ctx.fillStyle = pal.textMuted;
  ctx.font = `bold ${Math.round(5.6 * dpr)}px monospace`;
  ctx.fillText('0ms', x0, h - 2.8 * dpr);
  ctx.fillText(`A:${fmtSec(aSec)}`, Math.max(x0 + 22 * dpr, x1 - 12 * dpr), h - 2.8 * dpr);
  ctx.fillText(`D:${fmtSec(dSec)}`, Math.max(x1 + 22 * dpr, x2 - 12 * dpr), h - 2.8 * dpr);
  ctx.fillText(`TOT:${fmtSec(totSec)}`, Math.max(x2 + 24 * dpr, w - 46 * dpr), h - 2.8 * dpr);

  // HUD Superior
  const crvLabel = curveTension < -0.2 ? 'EXP' : (curveTension > 0.2 ? 'LOG' : 'LIN');
  const env2Names = ['CUT', 'PIT', 'WT'];
  const env2Lbl = Math.abs(env2Depth) > 0.02 ? `ENV2→${env2Names[state.params.env2_target || 0] || 'CUT'}` : 'ENV1 ONLY';
  ctx.fillStyle = pal.isDaylight ? pal.primaryHex : accHex;
  ctx.font = `bold ${Math.round(6.8 * dpr)}px monospace`;
  ctx.fillText(`ADSR (${crvLabel}) • S:${Math.round(sNorm * 100)}%`, 6 * dpr, 10 * dpr);
  ctx.fillStyle = currentUiSkin === 'dark' ? 'rgba(0, 235, 255, 0.88)' : pal.secondaryHex;
  ctx.fillText(env2Lbl, w - 58 * dpr, 10 * dpr);
}

function drawLfoOled() {
  const canvas = document.getElementById('oled-lfo');
  if (!canvas) return;
  const dpr = sizeOledCanvas(canvas);
  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;
  if (w <= 0 || h <= 0) return;

  const pal = getCanvasSkinPalette();
  const lfoHex = pal.isDaylight ? pal.primaryHex : (currentUiSkin === 'dark' ? (state.oledTheme === 'amber' ? '#ffaa00' : (state.oledTheme === 'emerald' ? '#00ebff' : '#10b981')) : pal.primaryHex);
  const lfoRgba = (a) => pal.isDaylight ? pal.primaryRgba(a) : (currentUiSkin === 'dark'
    ? (state.oledTheme === 'amber' ? `rgba(255, 170, 0, ${a})` : (state.oledTheme === 'emerald' ? `rgba(0, 235, 255, ${a})` : `rgba(16, 185, 129, ${a})`))
    : pal.primaryRgba(a));
  const ghostRgba = (a) => pal.isDaylight ? pal.secondaryRgba(a) : (currentUiSkin === 'dark'
    ? (state.oledTheme === 'amber' ? `rgba(0, 235, 255, ${a})` : `rgba(255, 170, 0, ${a})`)
    : pal.secondaryRgba(a));

  const bgGrad = ctx.createLinearGradient(0, 0, 0, h);
  bgGrad.addColorStop(0, pal.bgTop);
  bgGrad.addColorStop(1, pal.bgBot);
  ctx.fillStyle = bgGrad;
  ctx.fillRect(0, 0, w, h);

  const rulerH = 11 * dpr;
  const topPlotY = 13 * dpr;
  const botPlotY = h - rulerH;
  const midY = (topPlotY + botPlotY) * 0.5;
  const halfAmpH = (botPlotY - topPlotY) * 0.44;

  // 1. Sub-grilla musical polirrítmica (1/4, 2/4, 3/4, 1 BAR) y eje bipolar 0.0
  ctx.lineWidth = 1;
  const beatLabels = ['1/4', '2/4', '3/4', '1 BAR'];
  for (let i = 1; i <= 8; i++) {
    const gx = (w / 8) * i;
    ctx.strokeStyle = pal.gridLine;
    ctx.beginPath(); ctx.moveTo(gx, topPlotY); ctx.lineTo(gx, botPlotY); ctx.stroke();
  }
  ctx.strokeStyle = pal.zeroLine;
  ctx.beginPath(); ctx.moveTo(0, midY); ctx.lineTo(w, midY); ctx.stroke();

  const rateNorm = state.normalized.lfo_rate !== undefined ? state.normalized.lfo_rate : 0.32;
  const depthNorm = state.normalized.lfo_depth !== undefined ? state.normalized.lfo_depth : 0.30;
  const shape = state.params.lfo_shape || 0;
  const cycles = 1.5 + rateNorm * 4.5;
  const amp = Math.max(0.32, depthNorm) * halfAmpH;

  const evalLfoShape = (phaseNorm, sh = shape) => {
    const p = ((phaseNorm % 1.0) + 1.0) % 1.0;
    if (sh === 1) return 1.0 - 4.0 * Math.abs(p - 0.5); // TRI
    if (sh === 2) return 1.0 - 2.0 * p; // SAW
    if (sh === 3) return 2.0 * p - 1.0; // RAMP
    if (sh === 4) return p < 0.5 ? 1.0 : -1.0; // SQR
    if (sh === 5) { // S&H stepped
      const step = Math.floor(p * 8.0);
      return Math.sin(step * 12.9898 + 4.1414) >= 0 ? (0.45 + (step % 3) * 0.24) : -(0.40 + (step % 4) * 0.18);
    }
    return Math.sin(p * Math.PI * 2); // SINE
  };

  // 2. Curva secundaria LFO 2 (Armónico complementario desfasado)
  ctx.beginPath();
  for (let x = 0; x <= w; x += 2) {
    const t = x / w;
    const y2 = midY - evalLfoShape(t * (cycles * 0.5) - animPhase * 0.10, (shape + 1) % 5) * (amp * 0.62);
    if (x === 0) ctx.moveTo(x, y2);
    else ctx.lineTo(x, y2);
  }
  ctx.strokeStyle = ghostRgba(0.42);
  ctx.lineWidth = 1.1 * dpr;
  ctx.stroke();

  // 3. Relleno bipolar degradado y trazo principal de LFO 1
  const grad = ctx.createLinearGradient(0, topPlotY, 0, botPlotY);
  grad.addColorStop(0, lfoRgba(pal.isDaylight ? 0.16 : 0.36));
  grad.addColorStop(0.5, lfoRgba(0.02));
  grad.addColorStop(1, lfoRgba(pal.isDaylight ? 0.16 : 0.36));

  ctx.beginPath();
  ctx.moveTo(0, midY);
  for (let x = 0; x <= w; x += 2) {
    const t = x / w;
    const y = midY - evalLfoShape(t * cycles + animPhase * 0.16) * amp;
    ctx.lineTo(x, y);
  }
  ctx.lineTo(w, midY);
  ctx.closePath();
  ctx.fillStyle = grad;
  ctx.fill();

  ctx.beginPath();
  for (let x = 0; x <= w; x += 2) {
    const t = x / w;
    const y = midY - evalLfoShape(t * cycles + animPhase * 0.16) * amp;
    if (x === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.strokeStyle = lfoHex;
  ctx.lineWidth = 1.9 * dpr;
  ctx.shadowColor = lfoHex;
  ctx.shadowBlur = pal.isDaylight ? 0 : 7 * dpr;
  ctx.stroke();
  ctx.shadowBlur = 0;

  // 4. Nodos vectoriales en los cuartos de compás sobre la curva LFO
  for (let k = 1; k <= 4; k++) {
    const nt = (k - 0.5) / 4;
    const nx = nt * w;
    const ny = midY - evalLfoShape(nt * cycles + animPhase * 0.16) * amp;
    ctx.fillStyle = pal.nodeFill;
    ctx.strokeStyle = lfoHex;
    ctx.lineWidth = 1.3 * dpr;
    ctx.beginPath();
    ctx.arc(nx, ny, 2.3 * dpr, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
  }

  // 5. Láser vertical de fase LFO en vivo + Orbe brillante
  const cursorT = ((animPhase * 0.18) % 1.0 + 1.0) % 1.0;
  const cursorX = cursorT * w;
  const instVal = evalLfoShape(cursorT * cycles + animPhase * 0.16);
  const cursorY = midY - instVal * amp;

  ctx.save();
  ctx.strokeStyle = lfoRgba(0.45);
  ctx.lineWidth = 1.1 * dpr;
  ctx.beginPath(); ctx.moveTo(cursorX, topPlotY); ctx.lineTo(cursorX, botPlotY); ctx.stroke();

  ctx.fillStyle = pal.isDaylight ? pal.secondaryHex : '#ffffff';
  ctx.shadowColor = lfoHex;
  ctx.shadowBlur = pal.isDaylight ? 0 : 8 * dpr;
  ctx.beginPath();
  ctx.arc(cursorX, cursorY, 3.2 * dpr, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();

  // 6. Regla rítmica inferior (1/4 • 2/4 • 3/4 • 1 BAR) y HUD superior
  ctx.fillStyle = pal.rulerBg;
  ctx.fillRect(0, botPlotY, w, rulerH);
  ctx.strokeStyle = pal.gridLine;
  ctx.beginPath(); ctx.moveTo(0, botPlotY); ctx.lineTo(w, botPlotY); ctx.stroke();

  ctx.fillStyle = pal.textMuted;
  ctx.font = `bold ${Math.round(5.4 * dpr)}px monospace`;
  beatLabels.forEach((lbl, idx) => {
    const bx = ((idx + 1) / 4) * w - 18 * dpr;
    ctx.fillText(lbl, bx, h - 2.5 * dpr);
  });

  const shapeNames = ['SINE', 'TRI', 'SAW', 'RAMP', 'SQR', 'S&H'];
  const signStr = instVal >= 0 ? `+${(instVal * 100).toFixed(0)}%` : `${(instVal * 100).toFixed(0)}%`;
  ctx.fillStyle = lfoHex;
  ctx.font = `bold ${Math.round(6.6 * dpr)}px monospace`;
  ctx.fillText(`LFO 1 (${shapeNames[shape] || 'SINE'}) • ${formatKnobValue('lfo_rate', rateNorm)}`, 6 * dpr, 9.5 * dpr);
  ctx.fillStyle = pal.textPrimary;
  ctx.fillText(`MOD ${signStr}`, w - 48 * dpr, 9.5 * dpr);
}

const DEFAULT_ARP_PATTERNS = {
  0: [0.78, 0.45, 0.62, 0.88, 0.50, 0.70, 0.94, 0.48, 0.66, 0.82, 0.56, 0.90, 0.84, 0.52, 0.74, 0.92],
  1: [0.25, 0.40, 0.55, 0.70, 0.85, 1.00, 0.25, 0.40, 0.55, 0.70, 0.85, 1.00, 0.35, 0.58, 0.78, 0.96],
  2: [1.00, 0.85, 0.70, 0.55, 0.40, 0.25, 1.00, 0.85, 0.70, 0.55, 0.40, 0.25, 0.92, 0.72, 0.50, 0.30],
  3: [0.95, 0.48, 0.78, 0.92, 0.52, 0.86, 0.95, 0.48, 0.78, 0.92, 0.52, 0.86, 0.95, 0.50, 0.82, 0.90], // 6/8 SYNC
  4: [0.96, 0.55, 0.82, 0.48, 0.90, 0.96, 0.55, 0.82, 0.48, 0.90, 0.75, 0.60, 0.94, 0.54, 0.80, 0.66], // 5/8 SYNC
};

const ARP_PITCH_OFFSETS = [0, 4, 7, 12, 7, 12, 0, 3, 7, 10, 12, 15, 0, 7, 12, 19];

function getArpBarsForMode(mode) {
  if (!state.arpCustomPatterns) state.arpCustomPatterns = {};
  const targetLen = (state.params.arp_step_count === 16) ? 16 : 12;
  const fullDefault = DEFAULT_ARP_PATTERNS[mode] || DEFAULT_ARP_PATTERNS[3];
  if (!state.arpCustomPatterns[mode]) {
    state.arpCustomPatterns[mode] = fullDefault.slice(0, targetLen);
  } else if (state.arpCustomPatterns[mode].length !== targetLen) {
    if (targetLen === 16 && state.arpCustomPatterns[mode].length < 16) {
      while (state.arpCustomPatterns[mode].length < 16) {
        const idx = state.arpCustomPatterns[mode].length;
        state.arpCustomPatterns[mode].push(fullDefault[idx] || 0.80);
      }
    } else if (targetLen === 12 && state.arpCustomPatterns[mode].length > 12) {
      state.arpCustomPatterns[mode] = state.arpCustomPatterns[mode].slice(0, 12);
    }
  }
  if (mode === (state.params.arp_mode || 0)) {
    synthStore.setArpSteps(state.arpCustomPatterns[mode]);
  }
  return state.arpCustomPatterns[mode];
}

function drawArpOled() {
  const canvas = document.getElementById('oled-arp');
  if (!canvas) return;
  const dpr = sizeOledCanvas(canvas);
  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;
  if (w <= 0 || h <= 0) return;

  const pal = getCanvasSkinPalette();
  const arpHex = pal.isDaylight ? pal.primaryHex : (currentUiSkin === 'dark' ? (state.oledTheme === 'emerald' ? '#10b981' : '#ffaa00') : pal.primaryHex);
  const arpAccHex = pal.isDaylight ? pal.secondaryHex : (currentUiSkin === 'dark' ? (state.oledTheme === 'emerald' ? '#6ee7b7' : '#ffd166') : pal.secondaryHex);
  const arpCyanHex = pal.isDaylight ? pal.amberHex : (currentUiSkin === 'dark' ? (state.oledTheme === 'amber' ? '#ffffff' : '#00ebff') : pal.secondaryHex);

  const bgGrad = ctx.createLinearGradient(0, 0, 0, h);
  bgGrad.addColorStop(0, pal.bgTop);
  bgGrad.addColorStop(1, pal.bgBot);
  ctx.fillStyle = bgGrad;
  ctx.fillRect(0, 0, w, h);

  const mode = state.params.arp_mode || 0;
  const bars = getArpBarsForMode(mode);
  const isPlaying = state.activeNotes.size > 0 || (state.dspActiveVoices || 0) > 0;
  const dspSteps = (state.oddMetrics && typeof state.oddMetrics.arp_steps_triggered === 'number')
    ? state.oddMetrics.arp_steps_triggered
    : 0;
  const activeStep = (isPlaying && mode > 0 && dspSteps > 0)
    ? (dspSteps - 1) % bars.length
    : (isPlaying ? Math.floor(animPhase * 2.2) % bars.length : Math.floor(animPhase * 0.6) % bars.length);
  arpStepSource.set(activeStep % bars.length);

  const numBars = bars.length;
  const padX = 8 * dpr;
  const rulerH = 12 * dpr;
  const topPlotY = 16 * dpr;
  const botY = h - rulerH - 3 * dpr;
  const maxBarH = botY - topPlotY;
  const availW = w - padX * 2;
  const slotW = availW / numBars;
  const barW = Math.max(3.2 * dpr, slotW * 0.64);

  // 1. Grilla horizontal de velocidad (25%, 50%, 75%, 100%) y divisores de compás (6/8, 5/8 o 16-step 4/4)
  ctx.lineWidth = 1;
  [0.25, 0.5, 0.75, 1.0].forEach((lvl) => {
    const gy = botY - lvl * maxBarH;
    ctx.strokeStyle = pal.gridLine;
    ctx.beginPath(); ctx.moveTo(padX, gy); ctx.lineTo(w - padX, gy); ctx.stroke();
  });

  const groupSize = numBars === 16 ? 4 : (mode === 4 ? 5 : 3);
  for (let i = 0; i <= numBars; i += groupSize) {
    const gx = padX + i * slotW;
    ctx.strokeStyle = pal.isDaylight ? pal.secondaryRgba(0.16) : 'rgba(255, 170, 0, 0.18)';
    ctx.beginPath(); ctx.moveTo(gx, topPlotY - 2 * dpr); ctx.lineTo(gx, botY); ctx.stroke();
  }

  // 2. Columnas LED segmentadas + Línea de Gate + Ratchet sub-ticks + Nodos de Transposición por paso
  const pitchPts = [];
  const numSegs = 10;

  for (let i = 0; i < numBars; i++) {
    const val = bars[i];
    const bh = val * maxBarH;
    const bx = padX + i * slotW + (slotW - barW) * 0.5;
    const by = botY - bh;
    const isHit = i === activeStep || i === state.lastEditedArpStep;
    const isAccentBeat = (i % groupSize) === 0;
    const hasRatchet = numBars === 16 && (i === 3 || i === 7 || i === 11 || i === 15);

    // Halo de columna activa (Playhead Laser)
    if (isHit) {
      ctx.fillStyle = pal.primaryRgba(0.14);
      ctx.fillRect(padX + i * slotW, topPlotY - 2 * dpr, slotW, maxBarH + 4 * dpr);
    }

    // Segmentos LED discretos dentro de cada barra
    const segH = maxBarH / numSegs;
    const activeSegs = Math.max(1, Math.round(val * numSegs));
    for (let s = 0; s < numSegs; s++) {
      const sy = botY - (s + 1) * segH + 1 * dpr;
      const sh = Math.max(1 * dpr, segH - 1.4 * dpr);
      if (s < activeSegs) {
        if (isHit) {
          ctx.fillStyle = pal.isDaylight ? pal.primaryHex : (s === activeSegs - 1 ? '#ffffff' : '#ffe082');
        } else if (isAccentBeat) {
          ctx.fillStyle = arpHex;
        } else {
          ctx.fillStyle = pal.isDaylight ? pal.secondaryRgba(0.55) : pal.primaryRgba(0.65);
        }
      } else {
        ctx.fillStyle = pal.isDaylight ? 'rgba(23, 24, 26, 0.06)' : 'rgba(255, 170, 0, 0.06)';
      }
      ctx.fillRect(bx, sy, barW, sh);
    }

    // Tapa Peak-Hold luminosa en la cima del paso (dividida en 2 sub-pulsos si tiene Ratchet 2x)
    ctx.fillStyle = pal.isDaylight ? pal.secondaryHex : (isHit ? '#ffffff' : (hasRatchet ? arpCyanHex : arpAccHex));
    if (hasRatchet) {
      const subW = Math.max(1 * dpr, (barW - 1.2 * dpr) * 0.5);
      ctx.fillRect(bx, by, subW, Math.max(1.8 * dpr, 2.2 * dpr));
      ctx.fillRect(bx + subW + 1.2 * dpr, by, subW, Math.max(1.8 * dpr, 2.2 * dpr));
    } else {
      ctx.fillRect(bx, by, barW, Math.max(1.6 * dpr, 2.0 * dpr));
    }

    // Sub-barra horizontal de Gate Length en la base del paso
    const gateW = barW * (isAccentBeat ? 0.92 : 0.68);
    ctx.fillStyle = isHit ? arpCyanHex : pal.secondaryRgba(0.55);
    ctx.fillRect(bx, botY + 1.2 * dpr, gateW, 1.5 * dpr);

    // Coordenada del nodo de Pitch/Octava sobre la cima
    pitchPts.push({
      x: bx + barW * 0.5,
      y: Math.max(topPlotY + 3 * dpr, by - 3 * dpr),
      semi: ARP_PITCH_OFFSETS[i % ARP_PITCH_OFFSETS.length],
      isHit
    });
  }

  // 3. Curva de trayectoria melódica conectando las cimas de los 12/16 pasos
  ctx.beginPath();
  pitchPts.forEach((pt, idx) => {
    if (idx === 0) ctx.moveTo(pt.x, pt.y);
    else ctx.lineTo(pt.x, pt.y);
  });
  ctx.strokeStyle = currentUiSkin === 'dark' ? 'rgba(0, 235, 255, 0.55)' : pal.secondaryRgba(0.55);
  ctx.lineWidth = 1.1 * dpr;
  ctx.stroke();

  pitchPts.forEach((pt) => {
    ctx.fillStyle = pt.isHit ? pal.primaryHex : arpCyanHex;
    ctx.beginPath();
    ctx.arc(pt.x, pt.y, (pt.isHit ? 2.4 : 1.7) * dpr, 0, Math.PI * 2);
    ctx.fill();
  });

  // 4. Regla inferior numerada (01..12 o 01..16) y HUD superior
  ctx.fillStyle = pal.rulerBg;
  ctx.fillRect(0, h - rulerH, w, rulerH);
  ctx.strokeStyle = pal.gridLine;
  ctx.beginPath(); ctx.moveTo(0, h - rulerH); ctx.lineTo(w, h - rulerH); ctx.stroke();

  ctx.font = `bold ${Math.round(5.3 * dpr)}px monospace`;
  for (let i = 0; i < numBars; i++) {
    const bx = padX + i * slotW + slotW * 0.5 - 3.2 * dpr;
    const isHit = i === activeStep;
    ctx.fillStyle = isHit ? pal.primaryHex : (i % groupSize === 0 ? arpAccHex : pal.textMuted);
    const numStr = (i + 1) < 10 ? `0${i + 1}` : `${i + 1}`;
    ctx.fillText(numStr, bx, h - 2.8 * dpr);
  }

  const rateLbl = ARP_RATE_LABELS[state.params.arp_rate_div || 1] || '1/8';
  const titleMap = ['4/4 MANUAL', `ARP UP • ${numBars}ST`, `ARP DOWN • ${numBars}ST`, 'JOROPO 6/8 SYNC', 'MERENGUE 5/8 SYNC'];
  ctx.fillStyle = arpHex;
  ctx.font = `bold ${Math.round(6.6 * dpr)}px monospace`;
  ctx.fillText(`${titleMap[mode] || 'JOROPO 6/8'} (${rateLbl})`, 6 * dpr, 10 * dpr);
  ctx.fillStyle = pal.isDaylight ? pal.secondaryHex : arpCyanHex;
  const stepVelPct = Math.round((bars[activeStep] !== undefined ? bars[activeStep] : 0.85) * 100);
  ctx.fillText(`STEP ${activeStep + 1 < 10 ? '0' + (activeStep + 1) : activeStep + 1}/${numBars} • VEL ${stepVelPct}%`, w - 92 * dpr, 10 * dpr);
}

function drawReverbOled() {
  const canvas = document.getElementById('oled-reverb');
  if (!canvas) return;
  const dpr = sizeOledCanvas(canvas);
  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;
  if (w <= 0 || h <= 0) return;

  const pal = getCanvasSkinPalette();
  const primaryHex = pal.primaryHex;
  const secondaryHex = pal.secondaryHex;
  ctx.fillStyle = pal.bgTop;
  ctx.fillRect(0, 0, w, h);

  const driveNorm = state.normalized.drive !== undefined ? state.normalized.drive : 0.25;
  const ottNorm = state.normalized.ott !== undefined ? state.normalized.ott : 0.30;
  const mix = Math.max(0.15, state.normalized.space_mix !== undefined ? state.normalized.space_mix : 0.30);
  const isPlaying = state.activeNotes.size > 0 || (state.dspActiveVoices || 0) > 0;
  const rulerH = 11 * dpr;
  const topPlotY = 14 * dpr;
  const botPlotY = h - rulerH;
  const midY = (topPlotY + botPlotY) * 0.5;

  // =======================================================================
  // ZONA IZQUIERDA (26%): MEDIDORES MULTIBANDA OTT 3-BAND (LOW / MID / HIGH)
  // =======================================================================
  const ottZoneW = Math.round(w * 0.26);
  ctx.fillStyle = pal.isDaylight ? 'rgba(220, 216, 206, 0.72)' : 'rgba(7, 13, 22, 0.85)';
  ctx.fillRect(0, topPlotY, ottZoneW, botPlotY - topPlotY);
  ctx.strokeStyle = pal.gridLine;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(ottZoneW, topPlotY);
  ctx.lineTo(ottZoneW, botPlotY);
  ctx.stroke();

  const bands = [
    { lbl: 'L', level: Math.min(0.96, 0.38 + ottNorm * 0.45 + (isPlaying ? Math.sin(animPhase * 2.1) * 0.12 : 0.03)), color: primaryHex },
    { lbl: 'M', level: Math.min(0.96, 0.44 + ottNorm * 0.42 + (isPlaying ? Math.cos(animPhase * 2.7) * 0.12 : 0.04)), color: pal.isDaylight ? '#15803d' : '#10b981' },
    { lbl: 'H', level: Math.min(0.96, 0.34 + ottNorm * 0.50 + driveNorm * 0.15 + (isPlaying ? Math.sin(animPhase * 3.4) * 0.12 : 0.03)), color: pal.amberHex }
  ];
  const colW = (ottZoneW - 10 * dpr) / 3;
  const meterTop = topPlotY + 4 * dpr;
  const meterBot = botPlotY - 4 * dpr;
  const meterH = meterBot - meterTop;

  bands.forEach((b, idx) => {
    const bx = 5 * dpr + idx * colW + 1.5 * dpr;
    const bw = Math.max(4 * dpr, colW - 3 * dpr);
    ctx.fillStyle = pal.isDaylight ? 'rgba(205, 200, 190, 0.70)' : 'rgba(15, 23, 38, 0.95)';
    ctx.fillRect(bx, meterTop, bw, meterH);

    // Barra activa OTT Upward/Downward
    const bh = b.level * meterH;
    const by = meterBot - bh;
    ctx.fillStyle = b.color;
    ctx.fillRect(bx + 1 * dpr, by, bw - 2 * dpr, bh);

    // Corchete de umbral OTT (Threshold line)
    const threshY = meterBot - (0.55 + ottNorm * 0.25) * meterH;
    ctx.fillStyle = pal.isDaylight ? '#17181a' : '#ffffff';
    ctx.fillRect(bx, threshY, bw, 1.4 * dpr);
  });

  // =======================================================================
  // ZONA DERECHA (74%): ENVOLVENTE ESPACIAL CANYON IR + ARMÓNICOS DRIVE
  // =======================================================================
  const irStartX = ottZoneW + 4 * dpr;
  const irW = w - irStartX - 4 * dpr;
  const halfH = (botPlotY - topPlotY) * 0.44;

  // Grilla vertical de tiempo de reverberación (50ms, 250ms, 1.0s, 2.5s)
  [0.25, 0.5, 0.75].forEach((tn) => {
    const gx = irStartX + tn * irW;
    ctx.strokeStyle = pal.gridLine;
    ctx.beginPath(); ctx.moveTo(gx, topPlotY); ctx.lineTo(gx, botPlotY); ctx.stroke();
  });
  ctx.strokeStyle = pal.zeroLine;
  ctx.beginPath(); ctx.moveTo(irStartX, midY); ctx.lineTo(w, midY); ctx.stroke();

  // 1. Cono envolvente espacial estéreo (halo superior e inferior de decaimiento Canyon IR)
  ctx.beginPath();
  ctx.moveTo(irStartX, midY);
  for (let px = 0; px <= irW; px += 2) {
    const t = px / irW;
    const env = Math.exp(-t * (3.6 - mix * 2.2)) * (0.45 + mix * 0.55);
    const shimmer = 1.0 + Math.sin(t * 14.0 - animPhase * 0.8) * 0.12;
    ctx.lineTo(irStartX + px, midY - env * shimmer * halfH);
  }
  for (let px = irW; px >= 0; px -= 2) {
    const t = px / irW;
    const env = Math.exp(-t * (3.6 - mix * 2.2)) * (0.45 + mix * 0.55);
    const shimmer = 1.0 + Math.cos(t * 14.0 - animPhase * 0.8) * 0.12;
    ctx.lineTo(irStartX + px, midY + env * shimmer * halfH);
  }
  ctx.closePath();
  const coneGrad = ctx.createLinearGradient(irStartX, 0, w, 0);
  coneGrad.addColorStop(0, getOledPrimaryRgba(0.26));
  coneGrad.addColorStop(0.5, getOledSecondaryRgba(0.14));
  coneGrad.addColorStop(1, getOledPrimaryRgba(0.02));
  ctx.fillStyle = coneGrad;
  ctx.fill();

  // 2. Respuesta a Impulso Bipolar de alta densidad con saturación Drive en las crestas (batched)
  ctx.lineWidth = 1.15 * dpr;
  ctx.strokeStyle = getOledPrimaryRgba(0.85);
  ctx.beginPath();
  for (let px = 0; px < irW; px += 2) {
    const t = px / irW;
    const env = Math.exp(-t * (3.9 - mix * 2.3));
    const carrier = Math.sin(px * 0.68 + (isPlaying ? animPhase * 2.6 : 0)) * Math.cos(px * 0.27);
    const driveFold = Math.tanh(carrier * (1.2 + driveNorm * 2.8));
    const mag = Math.abs(driveFold) * env * halfH * (0.50 + mix * 0.50);
    const isHotPeak = mag > halfH * 0.48 && driveNorm > 0.15;
    if (!isHotPeak) {
      ctx.moveTo(irStartX + px, midY - mag);
      ctx.lineTo(irStartX + px, midY + mag);
    }
  }
  ctx.stroke();

  if (driveNorm > 0.15) {
    ctx.strokeStyle = secondaryHex;
    ctx.beginPath();
    for (let px = 0; px < irW; px += 2) {
      const t = px / irW;
      const env = Math.exp(-t * (3.9 - mix * 2.3));
      const carrier = Math.sin(px * 0.68 + (isPlaying ? animPhase * 2.6 : 0)) * Math.cos(px * 0.27);
      const driveFold = Math.tanh(carrier * (1.2 + driveNorm * 2.8));
      const mag = Math.abs(driveFold) * env * halfH * (0.50 + mix * 0.50);
      if (mag > halfH * 0.48) {
        ctx.moveTo(irStartX + px, midY - mag);
        ctx.lineTo(irStartX + px, midY + mag);
      }
    }
    ctx.stroke();
  }

  // 3. Regla inferior (OTT L/M/H + Tiempo de Cola IR) y HUD superior
  ctx.fillStyle = pal.rulerBg;
  ctx.fillRect(0, botPlotY, w, rulerH);
  ctx.strokeStyle = pal.gridLine;
  ctx.beginPath(); ctx.moveTo(0, botPlotY); ctx.lineTo(w, botPlotY); ctx.stroke();

  ctx.font = `bold ${Math.round(5.4 * dpr)}px monospace`;
  ctx.fillStyle = secondaryHex;
  ctx.fillText('L  M  H', 6 * dpr, h - 2.6 * dpr);
  ctx.fillStyle = pal.textMuted;
  ctx.fillText('50ms', irStartX + 4 * dpr, h - 2.6 * dpr);
  ctx.fillText('350ms', irStartX + irW * 0.36, h - 2.6 * dpr);
  ctx.fillText('1.2s', irStartX + irW * 0.68, h - 2.6 * dpr);
  ctx.fillText('2.8s', w - 18 * dpr, h - 2.6 * dpr);

  ctx.fillStyle = primaryHex;
  ctx.font = `bold ${Math.round(6.6 * dpr)}px monospace`;
  ctx.fillText(`3B-OTT ${Math.round(ottNorm * 100)}% • DRV ${Math.round(driveNorm * 100)}%`, 6 * dpr, 9.8 * dpr);
  ctx.fillStyle = secondaryHex;
  ctx.fillText(`CANYON ${Math.round(mix * 100)}%`, w - 52 * dpr, 9.8 * dpr);
}

function drawOscilloscope() {
  const canvas = document.getElementById('oscilloscope');
  if (!canvas) return;

  const dpr = sizeOledCanvas(canvas);
  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;
  if (w <= 0 || h <= 0) return;

  const pal = getCanvasSkinPalette();
  const primaryHex = pal.primaryHex;
  const secondaryHex = pal.secondaryHex;
  const isPlaying = state.activeNotes.size > 0 || (state.dspActiveVoices || 0) > 0;

  // Fondo OLED / Papel Técnico según skin activo
  ctx.fillStyle = pal.bgTop;
  ctx.fillRect(0, 0, w, h);

  // =========================================================================
  // MODO PROPUESTA A: TERRENO 3D ISOMÉTRICO TEPUI TABLE-MOUNTAIN (16 FRAMES)
  // =========================================================================
  if (state.wt3dMode && !state.wtDrawMode) {
    ctx.save();
    const wtPos = Math.min(1.0, Math.max(0.0, (state.normalized.wt_pos || 0) + (state.normalized.macro1 || 0) * 0.45));
    const wtWarp = Math.min(1.0, Math.max(0.0, (state.normalized.wt_warp || 0) + (state.normalized.macro2 || 0) * 0.45));
    const detune = state.normalized.unison_detune || 0.22;
    const fmB = state.params.fm_from_b || 0.0;

    // Proyección Isométrica 3D:
    // nx (0..1) = Fase Armónica (eje frontal izquierdo -> derecho)
    // nz (0..1) = Frame de Wavetable 1..16 (eje frontal -> trasero)
    // ny (-1..+1.5) = Elevación Topográfica de la Meseta Tepuy
    const originX = w * 0.14;
    const originY = h * 0.67;
    const vecXx = w * 0.46;
    const vecXy = h * 0.24;
    const vecZx = w * 0.34;
    const vecZy = -h * 0.34;
    const elevScale = h * 0.30;

    const isoProject = (nx, nz, ny = 0) => ({
      x: originX + nx * vecXx + nz * vecZx,
      y: originY + nx * vecXy + nz * vecZy - ny * elevScale
    });

    // 1. Rejilla de Piso Isométrica 3D (16x16) con Ejes Numerados (Propuesta A)
    ctx.lineWidth = Math.max(0.8, 0.8 * dpr);
    const gridSteps = 8;
    for (let g = 0; g <= gridSteps; g++) {
      const gn = g / gridSteps;
      const pA = isoProject(gn, 0, 0);
      const pB = isoProject(gn, 1, 0);
      ctx.strokeStyle = pal.isDaylight ? 'rgba(23, 24, 26, 0.13)' : 'rgba(0, 235, 255, 0.13)';
      ctx.beginPath(); ctx.moveTo(pA.x, pA.y); ctx.lineTo(pB.x, pB.y); ctx.stroke();

      const pC = isoProject(0, gn, 0);
      const pD = isoProject(1, gn, 0);
      ctx.strokeStyle = gn > 0.6
        ? pal.amberRgba(0.18)
        : (pal.isDaylight ? 'rgba(23, 24, 26, 0.13)' : 'rgba(0, 235, 255, 0.13)');
      ctx.beginPath(); ctx.moveTo(pC.x, pC.y); ctx.lineTo(pD.x, pD.y); ctx.stroke();
    }

    // Poste vertical izquierdo de elevación (+10, 0, -10 dB) y bordes isométricos inferiores
    const postBot = isoProject(0, 0, -0.22);
    const postTop = isoProject(0, 0, 0.85);
    ctx.strokeStyle = getOledPrimaryRgba(0.55);
    ctx.lineWidth = 1.1 * dpr;
    ctx.beginPath();
    ctx.moveTo(postTop.x, postTop.y);
    ctx.lineTo(postBot.x, postBot.y);
    ctx.lineTo(isoProject(1, 0, -0.22).x, isoProject(1, 0, -0.22).y);
    ctx.stroke();

    const rightCorner = isoProject(1, 0, -0.22);
    const backRight = isoProject(1, 1, -0.22);
    ctx.strokeStyle = getOledSecondaryRgba(0.55);
    ctx.beginPath();
    ctx.moveTo(rightCorner.x, rightCorner.y);
    ctx.lineTo(backRight.x, backRight.y);
    ctx.stroke();

    // Etiquetas numéricas isométricas (1..16 en ambos ejes y +10/0/-10 vertical)
    ctx.font = `${Math.round(5.6 * dpr)}px monospace`;
    ctx.fillStyle = getOledPrimaryRgba(0.75);
    ctx.fillText('+10', Math.max(1, postTop.x - 13 * dpr), postTop.y + 4 * dpr);
    ctx.fillText('0', Math.max(1, postTop.x - 8 * dpr), (postTop.y + postBot.y) * 0.5);
    ctx.fillText('-10', Math.max(1, postBot.x - 13 * dpr), postBot.y);

    const axisLabels = [1, 4, 8, 12, 16];
    axisLabels.forEach((num) => {
      const n = (num - 1) / 15;
      const ptLeft = isoProject(n, 0, -0.25);
      ctx.fillStyle = getOledPrimaryRgba(0.78);
      ctx.fillText(String(num), ptLeft.x - 3 * dpr, Math.min(h - 1.5 * dpr, ptLeft.y + 6 * dpr));

      const ptRight = isoProject(1, n, -0.25);
      ctx.fillStyle = getOledSecondaryRgba(0.78);
      ctx.fillText(String(num), ptRight.x + 2 * dpr, Math.min(h - 1.5 * dpr, ptRight.y + 5 * dpr));
    });

    // 2. Función de Elevación 3D del Terreno Tepuy (Meseta Monte Roraima + Armónicos Wavetable)
    const numSlices = 16;
    const numSamples = 52;
    const activeSliceFloat = wtPos * (numSlices - 1);
    const livePulse = isPlaying ? Math.sin(animPhase * 2.2) * 0.08 : Math.sin(animPhase * 0.45) * 0.025;

    const evalTepuiTerrain = (nx, nz) => {
      // Distancia elíptica al núcleo de la meseta Tepuy (Mount Roraima plateau)
      const dx = (nx - 0.50) * 2.15;
      const dz = (nz - 0.48) * 2.05;
      const radial = Math.sqrt(dx * dx + dz * dz + 0.14 * Math.sin(nx * 9.0 + nz * 7.0));
      // Acantilados verticales escarpados del Tepuy (transición sigmoidal abrupta)
      const cliffWall = 1.0 / (1.0 + Math.exp((radial - (0.64 + wtWarp * 0.12)) * 15.0));
      // Rugosidad estratificada de la cumbre tabular del Tepuy
      const summitCrags = (
        0.11 * Math.cos(nx * 19.0 + nz * 11.0 + wtWarp * 4.0) +
        0.07 * Math.sin(nx * 31.0 - nz * 17.0)
      ) * cliffWall;
      const mesaElevation = cliffWall * (0.72 + wtWarp * 0.22) + summitCrags;

      // Ondas acústicas de la tabla de ondas propagándose alrededor y sobre la meseta
      const warpedX = Math.pow(nx, 1.0 - wtWarp * 0.45);
      const phase = warpedX * Math.PI * 2 * (2.0 + nz * 3.0);
      const customVal = (Array.isArray(state.customWtCycle) && state.customWtCycle.length > 0)
        ? state.customWtCycle[Math.min(state.customWtCycle.length - 1, Math.floor(nx * state.customWtCycle.length))] * 0.32
        : 0;
      const harmonicWave = (
        Math.sin(phase + animPhase * (isPlaying ? 1.4 : 0.25)) * 0.20 +
        Math.sin(phase * (2.0 + fmB * 3.0) - nz * 4.0) * (0.12 + detune * 0.10) +
        customVal
      ) * (1.0 - cliffWall * 0.38);

      return Math.max(-0.22, Math.min(1.18, mesaElevation + harmonicWave + livePulse));
    };

    // Precalcular malla 3D [16][53] para oclusión Painter's Algorithm y líneas transversales
    const mesh = [];
    for (let s = 0; s < numSlices; s++) {
      const nz = s / (numSlices - 1);
      const row = [];
      for (let i = 0; i <= numSamples; i++) {
        const nx = i / numSamples;
        const ny = evalTepuiTerrain(nx, nz);
        const pt = isoProject(nx, nz, ny);
        const basePt = isoProject(nx, nz, -0.18);
        row.push({ nx, nz, ny, x: pt.x, y: pt.y, baseX: basePt.x, baseY: basePt.y });
      }
      mesh.push(row);
    }

    // 3. Renderizado Back-to-Front (s = 15 atrás -> s = 0 adelante) con Oclusión 3D y Bruma
    const curtainBot = pal.isDaylight ? 'rgba(232, 229, 221, 0.92)' : 'rgba(4, 8, 15, 0.90)';
    const activeCurtainGrad = ctx.createLinearGradient(0, originY - elevScale, 0, originY + h * 0.25);
    activeCurtainGrad.addColorStop(0, getOledSecondaryRgba(pal.isDaylight ? 0.24 : 0.42));
    activeCurtainGrad.addColorStop(1, curtainBot);

    const warmCurtainGrad = ctx.createLinearGradient(0, originY - elevScale, 0, originY + h * 0.25);
    warmCurtainGrad.addColorStop(0, pal.amberRgba(0.22));
    warmCurtainGrad.addColorStop(1, curtainBot);

    const coolCurtainGrad = ctx.createLinearGradient(0, originY - elevScale, 0, originY + h * 0.25);
    coolCurtainGrad.addColorStop(0, getOledPrimaryRgba(0.24));
    coolCurtainGrad.addColorStop(1, curtainBot);

    for (let s = numSlices - 1; s >= 0; s--) {
      const nz = s / (numSlices - 1);
      const row = mesh[s];
      const distToActive = Math.abs(s - activeSliceFloat);
      const isActiveSlice = distToActive < 0.55;
      const isWarmZone = nz > 0.58; // Frames 10..16 en Ámbar dorado, Frames 1..9 en Cyan/Braun primario

      // Cortina de oclusión 3D bajo la cresta (Painter's Algorithm)
      ctx.beginPath();
      ctx.moveTo(row[0].baseX, row[0].baseY);
      for (let i = 0; i <= numSamples; i++) {
        ctx.lineTo(row[i].x, row[i].y);
      }
      ctx.lineTo(row[numSamples].baseX, row[numSamples].baseY);
      ctx.closePath();

      ctx.fillStyle = isActiveSlice ? activeCurtainGrad : (isWarmZone ? warmCurtainGrad : coolCurtainGrad);
      ctx.fill();

      // Líneas de malla topográfica transversal (unen slice s con s+1 sobre la meseta del Tepuy)
      if (s < numSlices - 1) {
        const backRow = mesh[s + 1];
        ctx.strokeStyle = isWarmZone ? pal.amberRgba(0.24) : getOledPrimaryRgba(0.24);
        ctx.lineWidth = 0.75 * dpr;
        ctx.beginPath();
        for (let i = 2; i < numSamples; i += 3) {
          if (row[i].ny > 0.22 || backRow[i].ny > 0.22) {
            ctx.moveTo(row[i].x, row[i].y);
            ctx.lineTo(backRow[i].x, backRow[i].y);
          }
        }
        ctx.stroke();
      }

      // Bruma atmosférica volumétrica en la base de los acantilados del Tepuy (Frame central)
      if (s === 7) {
        const mistCenter = isoProject(0.50, 0.46, 0.24);
        const mistGrad = ctx.createRadialGradient(
          mistCenter.x, mistCenter.y, 4 * dpr,
          mistCenter.x, mistCenter.y, w * 0.26
        );
        mistGrad.addColorStop(0, pal.isDaylight ? 'rgba(255, 255, 255, 0.44)' : 'rgba(224, 248, 255, 0.26)');
        mistGrad.addColorStop(0.45, getOledPrimaryRgba(0.14));
        mistGrad.addColorStop(1, pal.isDaylight ? 'rgba(239, 237, 230, 0)' : 'rgba(3, 6, 10, 0)');
        ctx.fillStyle = mistGrad;
        ctx.fillRect(0, 0, w, h);
      }

      // Cresta luminosa de la rodaja s
      ctx.beginPath();
      for (let i = 0; i <= numSamples; i++) {
        if (i === 0) ctx.moveTo(row[i].x, row[i].y);
        else ctx.lineTo(row[i].x, row[i].y);
      }
      if (isActiveSlice) {
        ctx.strokeStyle = pal.isDaylight ? '#17181a' : '#ffffff';
        ctx.lineWidth = 2.1 * dpr;
        ctx.shadowColor = pal.isDaylight ? primaryHex : secondaryHex;
        ctx.shadowBlur = 8 * dpr;
      } else {
        ctx.strokeStyle = isWarmZone
          ? pal.amberRgba((0.36 + (1.0 - nz) * 0.42).toFixed(2))
          : getOledPrimaryRgba((0.36 + (1.0 - nz) * 0.50).toFixed(2));
        ctx.lineWidth = 1.15 * dpr;
        ctx.shadowBlur = 0;
      }
      ctx.stroke();
      ctx.shadowBlur = 0;
    }

    // HUD 3D Superior Derecho indicando Frame Activo, Granular Mode y Morfología
    const activeFrameNum = Math.min(16, Math.max(1, Math.round(wtPos * 15) + 1));
    const granMode = state.params.granular_mode || 0;
    if (granMode > 0) {
      const anchorNorm = Math.max(0.1, Math.min(0.9, wtPos));
      for (let g = 0; g < 4; g++) {
        const gPhase = ((animPhase * (granMode === 2 ? 0.55 : 0.35) + g * 0.25) % 1.0 + 1.0) % 1.0;
        const hannWin = 0.5 * (1.0 - Math.cos(gPhase * Math.PI * 2));
        const gx = Math.max(0.05, Math.min(0.95, anchorNorm + (gPhase - 0.5) * (granMode === 2 ? 0.08 : 0.24)));
        const gz = 0.20 + (g / 3) * 0.60;
        const gpt = isoProject(gx, gz, 0.35 + hannWin * 0.38);
        ctx.fillStyle = granMode === 2 ? `rgba(56, 189, 248, ${(0.25 + hannWin * 0.65).toFixed(2)})` : pal.amberRgba((0.25 + hannWin * 0.65).toFixed(2));
        ctx.beginPath();
        ctx.arc(gpt.x, gpt.y, (2.2 + hannWin * 3.8) * dpr, 0, Math.PI * 2);
        ctx.fill();
      }
    }
    ctx.fillStyle = secondaryHex;
    ctx.font = `bold ${Math.round(6.4 * dpr)}px monospace`;
    const granTag = granMode === 2 ? ' • FREEZE' : (granMode === 1 ? ' • 4-GRAIN' : '');
    ctx.fillText(`3D TEPUI MESA • FRM ${activeFrameNum}/16${granTag}`, w - 126 * dpr, 20 * dpr);
    ctx.restore();
    return;
  }

  // =========================================================================
  // MODO 2D: EDICIÓN DE WAVETABLE FREEHAND (WT DRAW) O SAMPLER 2D (S/E/L1/L2)
  // =========================================================================
  const centerY = h / 2;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, centerY);
  ctx.lineTo(w, centerY);
  ctx.strokeStyle = pal.zeroLine;
  ctx.stroke();

  ctx.strokeStyle = pal.gridLine;
  for (let i = 1; i < 12; i++) {
    const x = (w / 12) * i;
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, h);
    ctx.stroke();
  }

  const pts = (state.wtDrawMode && Array.isArray(state.customWtCycle) && state.customWtCycle.length > 1)
    ? state.customWtCycle
    : state.waveform;
  if (!pts || pts.length < 2) return;

  const wobble = isPlaying ? Math.sin(animPhase) * 0.08 : 0;
  const grad = ctx.createLinearGradient(0, 0, 0, h);
  grad.addColorStop(0, getOledPrimaryRgba(0.24));
  grad.addColorStop(0.5, getOledPrimaryRgba(0.04));
  grad.addColorStop(1, getOledPrimaryRgba(0.24));

  // En modo WT DRAW mostramos además barras armónicas verticales estilo Serum Wavetable Editor
  if (state.wtDrawMode) {
    const barSlot = w / pts.length;
    ctx.fillStyle = getOledPrimaryRgba(0.22);
    for (let i = 0; i < pts.length; i++) {
      const bx = i * barSlot;
      const val = pts[i];
      const by = centerY - val * (centerY - 8 * dpr);
      ctx.fillRect(bx, Math.min(centerY, by), Math.max(1, barSlot - 1 * dpr), Math.abs(by - centerY));
    }
  }

  ctx.beginPath();
  ctx.moveTo(0, centerY);
  for (let i = 0; i < pts.length; i++) {
    const x = (i / (pts.length - 1)) * w;
    const sampleVal = pts[i] * (1.0 + wobble);
    const y = centerY - sampleVal * (centerY - 10);
    ctx.lineTo(x, y);
  }
  ctx.lineTo(w, centerY);
  ctx.fillStyle = grad;
  ctx.fill();

  ctx.beginPath();
  for (let i = 0; i < pts.length; i++) {
    const x = (i / (pts.length - 1)) * w;
    const sampleVal = pts[i] * (1.0 + wobble);
    const y = centerY - sampleVal * (centerY - 10);
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.strokeStyle = isPlaying ? (pal.isDaylight ? '#17181a' : '#ffffff') : primaryHex;
  ctx.lineWidth = (isPlaying ? 2.0 : 1.7) * dpr;
  ctx.shadowColor = primaryHex;
  ctx.shadowBlur = (isPlaying ? 10 : 5) * dpr;
  ctx.stroke();
  ctx.shadowBlur = 0;

  if (state.wtDrawMode) {
    ctx.fillStyle = secondaryHex;
    ctx.font = `bold ${Math.round(6.8 * dpr)}px monospace`;
    ctx.fillText('64-POINT HARMONIC DRAW • DOBLE CLIC RESET', 6 * dpr, h - 4 * dpr);
    return;
  }

  // --- OVERLAY INTERACTIVO DE TRIM (START / END) Y LOOP (L1 / L2) EN MODO 2D SAMPLER ---
  const r = state.region || { trim_start_norm: 0, trim_end_norm: 1, loop_enabled: true, loop_start_norm: 0.2, loop_end_norm: 0.85 };
  const trimStartX = Math.round(r.trim_start_norm * w);
  const trimEndX = Math.round(r.trim_end_norm * w);
  const loopStartX = Math.round(r.loop_start_norm * w);
  const loopEndX = Math.round(r.loop_end_norm * w);

  ctx.fillStyle = pal.isDaylight ? 'rgba(23, 24, 26, 0.16)' : 'rgba(2, 4, 8, 0.72)';
  if (trimStartX > 0) ctx.fillRect(0, 0, trimStartX, h);
  if (trimEndX < w) ctx.fillRect(trimEndX, 0, w - trimEndX, h);

  if (r.loop_enabled && loopEndX > loopStartX) {
    ctx.fillStyle = pal.amberRgba(0.11);
    ctx.fillRect(loopStartX, 0, loopEndX - loopStartX, h);

    ctx.save();
    ctx.strokeStyle = pal.amberHex;
    ctx.lineWidth = Math.max(1.2, 1.2 * dpr);
    ctx.setLineDash([4 * dpr, 3 * dpr]);
    ctx.beginPath();
    ctx.moveTo(loopStartX, 0); ctx.lineTo(loopStartX, h);
    ctx.moveTo(loopEndX, 0); ctx.lineTo(loopEndX, h);
    ctx.stroke();
    ctx.restore();

    ctx.fillStyle = pal.amberHex;
    const flagW = 14 * dpr;
    const flagH = 11 * dpr;
    ctx.fillRect(loopStartX, h - flagH, flagW, flagH);
    ctx.fillRect(Math.max(0, loopEndX - flagW), h - flagH, flagW, flagH);
    ctx.fillStyle = pal.isDaylight ? '#ffffff' : '#05070b';
    ctx.font = `bold ${Math.round(7.5 * dpr)}px monospace`;
    ctx.fillText('L1', loopStartX + 2 * dpr, h - 2.5 * dpr);
    ctx.fillText('L2', Math.max(0, loopEndX - flagW) + 2 * dpr, h - 2.5 * dpr);
  }

  const trimColor = pal.isDaylight ? '#15803d' : '#10b981';
  ctx.strokeStyle = trimColor;
  ctx.lineWidth = Math.max(1.5, 1.5 * dpr);
  ctx.beginPath();
  ctx.moveTo(trimStartX, 0); ctx.lineTo(trimStartX, h);
  ctx.moveTo(trimEndX, 0); ctx.lineTo(trimEndX, h);
  ctx.stroke();

  const tFlagW = 11 * dpr;
  const tFlagH = 11 * dpr;
  ctx.fillStyle = trimColor;
  ctx.fillRect(trimStartX, 0, tFlagW, tFlagH);
  ctx.fillRect(Math.max(0, trimEndX - tFlagW), 0, tFlagW, tFlagH);
  ctx.fillStyle = pal.isDaylight ? '#ffffff' : '#04120c';
  ctx.font = `bold ${Math.round(7.5 * dpr)}px monospace`;
  ctx.fillText('S', trimStartX + 2.5 * dpr, 8.5 * dpr);
  ctx.fillText('E', Math.max(0, trimEndX - tFlagW) + 2.5 * dpr, 8.5 * dpr);

  const phNorm = state.playheadNorm || 0;
  if (isPlaying || phNorm > 0.001) {
    const effPlayhead = phNorm > 0.001
      ? phNorm
      : (r.trim_start_norm + ((animPhase * 0.08) % Math.max(0.05, r.trim_end_norm - r.trim_start_norm)));
    const phX = Math.round(Math.max(0, Math.min(1, effPlayhead)) * w);

    ctx.save();
    ctx.strokeStyle = pal.isDaylight ? '#17181a' : '#ffffff';
    ctx.lineWidth = Math.max(1.6, 1.6 * dpr);
    ctx.shadowColor = primaryHex;
    ctx.shadowBlur = 8 * dpr;
    ctx.beginPath();
    ctx.moveTo(phX, 0);
    ctx.lineTo(phX, h);
    ctx.stroke();
    ctx.restore();
  }
}

function updateVuMeters() {
  const hasUiNotes = state.activeNotes.size > 0;
  const uiTargetL = hasUiNotes ? Math.min(0.95, 0.35 + state.activeNotes.size * 0.15 + (Math.sin(animPhase * 2) * 0.08)) : 0.0;
  const uiTargetR = hasUiNotes ? Math.min(0.95, 0.33 + state.activeNotes.size * 0.15 + (Math.cos(animPhase * 2) * 0.08)) : 0.0;

  const targetL = Math.min(1.0, Math.max(state.dspPeakL || 0.0, uiTargetL));
  const targetR = Math.min(1.0, Math.max(state.dspPeakR || 0.0, uiTargetR));

  // Decaimiento entre lecturas de telemetría DSP
  state.dspPeakL = Math.max(0, (state.dspPeakL || 0) * 0.92);
  state.dspPeakR = Math.max(0, (state.dspPeakR || 0) * 0.92);

  // Ataque instantáneo, caída exponencial suave
  if (targetL > state.vuLevelL) state.vuLevelL = targetL;
  else state.vuLevelL = Math.max(0, state.vuLevelL - 0.04);

  if (targetR > state.vuLevelR) state.vuLevelR = targetR;
  else state.vuLevelR = Math.max(0, state.vuLevelR - 0.04);

  // Peak-Hold con retención temporal y caída lenta
  state.vuPeakHoldL = state.vuPeakHoldL || 0;
  state.vuPeakHoldR = state.vuPeakHoldR || 0;
  state.vuPeakTimerL = state.vuPeakTimerL || 0;
  state.vuPeakTimerR = state.vuPeakTimerR || 0;

  if (state.vuLevelL >= state.vuPeakHoldL) {
    state.vuPeakHoldL = state.vuLevelL;
    state.vuPeakTimerL = 26;
  } else if (state.vuPeakTimerL > 0) {
    state.vuPeakTimerL--;
  } else {
    state.vuPeakHoldL = Math.max(0, state.vuPeakHoldL - 0.015);
  }

  if (state.vuLevelR >= state.vuPeakHoldR) {
    state.vuPeakHoldR = state.vuLevelR;
    state.vuPeakTimerR = 26;
  } else if (state.vuPeakTimerR > 0) {
    state.vuPeakTimerR--;
  } else {
    state.vuPeakHoldR = Math.max(0, state.vuPeakHoldR - 0.015);
  }

  vuMeterSource.set({ levels: [state.vuLevelL, state.vuLevelR], peaks: [state.vuPeakHoldL, state.vuPeakHoldR] });
}

function initInteractiveOleds() {
  // 0. 64-Band Real-Time FFT Spectrum Toggle button on SVF Filter Header + Keytrack Toggle
  const btnSvfFft = document.getElementById('btn-svf-fft-toggle');
  if (btnSvfFft) {
    state.svfFftEnabled = state.svfFftEnabled !== false;
    btnSvfFft.classList.toggle('active', state.svfFftEnabled);
    btnSvfFft.addEventListener('click', () => {
      state.svfFftEnabled = !state.svfFftEnabled;
      btnSvfFft.classList.toggle('active', state.svfFftEnabled);
    });
  }

  const btnSvfKey = document.getElementById('btn-svf-keytrack');
  if (btnSvfKey) {
    const KEY_STEPS = [
      { label: 'KEY:50%', val: 0.50 },
      { label: 'KEY:100%', val: 1.00 },
      { label: 'KEY:OFF', val: 0.00 }
    ];
    let keyIdx = 0;
    btnSvfKey.addEventListener('click', () => {
      keyIdx = (keyIdx + 1) % KEY_STEPS.length;
      const step = KEY_STEPS[keyIdx];
      btnSvfKey.innerText = step.label;
      sendIpc('kt,' + step.val.toFixed(2));
    });
  }

  // Botones de Segunda Envolvente Dedicada (MOD ENV 2) y Curvatura de Etapas (EXP/LIN/LOG)
  const btnEnv2 = document.getElementById('btn-env2-target');
  if (btnEnv2) {
    const ENV2_PRESETS = [
      { target: 0, depth: 0.65 },
      { target: 1, depth: 0.55 },
      { target: 2, depth: 0.72 },
      { target: 0, depth: 0.00 }
    ];
    let env2StepIdx = 0;
    btnEnv2.addEventListener('click', () => {
      env2StepIdx = (env2StepIdx + 1) % ENV2_PRESETS.length;
      const p = ENV2_PRESETS[env2StepIdx];
      updateEnv2UI(p.target, p.depth);
      sendIpc(`env2,${p.target},${p.depth.toFixed(2)}`);
      oledDirtyCache.adsrSig = '';
      drawAdsrOled();
    });
  }

  const btnEnvCurve = document.getElementById('btn-env-curve');
  if (btnEnvCurve) {
    const CURVE_STEPS = [-0.65, 0.00, 0.65];
    let curveIdx = 0;
    btnEnvCurve.addEventListener('click', () => {
      curveIdx = (curveIdx + 1) % CURVE_STEPS.length;
      const nextCrv = CURVE_STEPS[curveIdx];
      updateEnvCurveUI(nextCrv);
      sendIpc(`envcrv,${nextCrv.toFixed(2)}`);
      oledDirtyCache.adsrSig = '';
      drawAdsrOled();
    });
  }

  // Botones de Subdivisión Rítmica (1/4, 1/8, 1/16, 1/8T, 1/16D) y 12/16 Pasos del Secuenciador
  const btnArpRate = document.getElementById('btn-arp-rate');
  if (btnArpRate) {
    btnArpRate.addEventListener('click', () => {
      const nextRate = ((state.params.arp_rate_div !== undefined ? state.params.arp_rate_div : 1) + 1) % 5;
      updateArpRateUI(nextRate);
      sendIpc(`arprate,${nextRate}`);
      oledDirtyCache.arpSig = '';
      drawArpOled();
    });
  }

  const btnArpSteps = document.getElementById('btn-arp-steps');
  if (btnArpSteps) {
    btnArpSteps.addEventListener('click', () => {
      const nextSteps = (state.params.arp_step_count === 16) ? 12 : 16;
      updateArpStepsCountUI(nextSteps);
      sendIpc(`arpseq,${nextSteps}`);
      syncArpPatternToDsp(state.params.arp_mode || 0);
      oledDirtyCache.arpSig = '';
      drawArpOled();
    });
  }

  const lfoCanvas = document.getElementById('oled-lfo');
  if (lfoCanvas) {
    lfoCanvas.style.cursor = 'pointer';
    lfoCanvas.addEventListener('click', () => {
      const btnLfoShape = document.getElementById('btn-lfo-shape');
      if (btnLfoShape) btnLfoShape.click();
    });
  }

  // 1. 2D Filter Node Manipulation on #oled-svf (Cutoff X, Resonance Y)
  const svfCanvas = document.getElementById('oled-svf');
  if (svfCanvas) {
    const applySvf2D = (e) => {
      const rect = svfCanvas.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;
      const normX = Math.max(0.0, Math.min(1.0, (e.clientX - rect.left) / rect.width));
      const normY = Math.max(0.0, Math.min(1.0, 1.0 - (e.clientY - rect.top) / rect.height));
      state.normalized.cutoff = normX;
      state.normalized.resonance = normY;
      updateKnobUI('cutoff', normX);
      updateKnobUI('resonance', normY);
      sendFastParam('cutoff', normX);
      sendFastParam('resonance', normY);
      showParamHudTooltip(
        svfCanvas,
        'SVF 2D',
        `${formatKnobValue('cutoff', normX)} • Q ${formatKnobValue('resonance', normY)}`
      );
    };

    svfCanvas.addEventListener('mousedown', (e) => {
      state.isDraggingSvfOled = true;
      state.isDraggingKnob = true;
      applySvf2D(e);
      e.preventDefault();
    });
    window.addEventListener('mousemove', (e) => {
      if (state.isDraggingSvfOled) applySvf2D(e);
    });
    window.addEventListener('mouseup', () => {
      if (state.isDraggingSvfOled) {
        state.isDraggingSvfOled = false;
        state.isDraggingKnob = false;
        hideParamHudTooltip();
      }
    });
    svfCanvas.addEventListener('dblclick', () => {
      state.normalized.cutoff = 1.0;
      state.normalized.resonance = 0.061;
      updateKnobUI('cutoff', 1.0);
      updateKnobUI('resonance', 0.061);
      sendFastParam('cutoff', 1.0);
      sendFastParam('resonance', 0.061);
    });
  }

  // 2. 2D Envelope Stage Node Manipulation on #oled-adsr (Attack, Decay, Sustain, Release)
  const adsrCanvas = document.getElementById('oled-adsr');
  if (adsrCanvas) {
    const pickAdsrNode = (normX) => {
      const lay = state.adsrNodesLayout || { x1: 0.18, x2: 0.36, x3: 0.56, x4: 0.78 };
      const candidates = [
        { id: 'attack', x: lay.x1 },
        { id: 'decay', x: lay.x2 },
        { id: 'sustain', x: lay.x3 },
        { id: 'release', x: lay.x4 }
      ];
      let best = 'decay';
      let minDist = 999;
      candidates.forEach((c) => {
        const d = Math.abs(normX - c.x);
        if (d < minDist) {
          minDist = d;
          best = c.id;
        }
      });
      return best;
    };

    const applyAdsr2D = (e) => {
      const rect = adsrCanvas.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;
      const normX = Math.max(0.0, Math.min(1.0, (e.clientX - rect.left) / rect.width));
      const normY = Math.max(0.0, Math.min(1.0, 1.0 - (e.clientY - rect.top) / rect.height));
      const node = state.activeAdsrDragNode || 'decay';
      const lay = state.adsrNodesLayout || { x0: 0.03, x1: 0.18, x2: 0.36, x3: 0.56 };

      if (node === 'attack') {
        const aVal = Math.max(0.0, Math.min(1.0, (normX - 0.04) / 0.28));
        state.normalized.attack = aVal;
        updateKnobUI('attack', aVal);
        sendFastParam('attack', aVal);
        showParamHudTooltip(adsrCanvas, 'ATTACK 2D', formatKnobValue('attack', aVal));
      } else if (node === 'decay') {
        const dVal = Math.max(0.0, Math.min(1.0, (normX - lay.x1 - 0.02) / 0.25));
        const sVal = Math.max(0.0, Math.min(1.0, normY));
        state.normalized.decay = dVal;
        state.normalized.sustain = sVal;
        updateKnobUI('decay', dVal);
        updateKnobUI('sustain', sVal);
        sendFastParam('decay', dVal);
        sendFastParam('sustain', sVal);
        showParamHudTooltip(
          adsrCanvas,
          'DECAY / SUS',
          `${formatKnobValue('decay', dVal)} • ${formatKnobValue('sustain', sVal)}`
        );
      } else if (node === 'sustain') {
        const sVal = Math.max(0.0, Math.min(1.0, normY));
        state.normalized.sustain = sVal;
        updateKnobUI('sustain', sVal);
        sendFastParam('sustain', sVal);
        showParamHudTooltip(adsrCanvas, 'SUSTAIN 2D', formatKnobValue('sustain', sVal));
      } else if (node === 'release') {
        const rVal = Math.max(0.0, Math.min(1.0, (normX - lay.x3 - 0.02) / 0.28));
        state.normalized.release = rVal;
        updateKnobUI('release', rVal);
        sendFastParam('release', rVal);
        showParamHudTooltip(adsrCanvas, 'RELEASE 2D', formatKnobValue('release', rVal));
      }
    };

    adsrCanvas.addEventListener('mousedown', (e) => {
      const rect = adsrCanvas.getBoundingClientRect();
      if (rect.width <= 0) return;
      const normX = Math.max(0.0, Math.min(1.0, (e.clientX - rect.left) / rect.width));
      state.activeAdsrDragNode = pickAdsrNode(normX);
      state.isDraggingKnob = true;
      applyAdsr2D(e);
      e.preventDefault();
    });
    window.addEventListener('mousemove', (e) => {
      if (state.activeAdsrDragNode) applyAdsr2D(e);
    });
    window.addEventListener('mouseup', () => {
      if (state.activeAdsrDragNode) {
        state.activeAdsrDragNode = null;
        state.isDraggingKnob = false;
        hideParamHudTooltip();
      }
    });
  }

  // 3. Interactive Step Velocity/Accent Sculpting on #oled-arp
  const arpCanvas = document.getElementById('oled-arp');
  if (arpCanvas) {
    let draggingArp = false;
    const paintArpStep = (e) => {
      const rect = arpCanvas.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;
      const normX = Math.max(0.0, Math.min(0.999, (e.clientX - rect.left) / rect.width));
      const normY = Math.max(0.0, Math.min(1.0, (e.clientY - rect.top) / rect.height));
      const mode = state.params.arp_mode || 0;
      const bars = getArpBarsForMode(mode);
      const stepIdx = Math.max(0, Math.min(bars.length - 1, Math.floor(normX * bars.length)));
      const accent = Math.max(0.05, Math.min(1.0, 1.0 - (normY - 0.12) / 0.76));
      bars[stepIdx] = accent;
      if (mode === 0) {
        // Si el usuario dibuja su curva de velocidad mientras ARP está en OFF, propagarla a los modos activos
        [1, 2, 3, 4].forEach((m) => {
          getArpBarsForMode(m)[stepIdx] = accent;
        });
      }
      state.lastEditedArpStep = stepIdx;
      state.currentVelocity = accent;
      state.lastPlayedVelocity = accent;
      sendIpc('arpstep,' + stepIdx + ',' + accent.toFixed(3));
      syncArpPatternToDsp(mode);
      const velBadge = document.getElementById('vel-badge');
      if (velBadge) velBadge.innerText = `${Math.round(accent * 100)}%`;
      showParamHudTooltip(arpCanvas, `STEP ${stepIdx + 1} VEL`, `${Math.round(accent * 100)}%`);
    };

    arpCanvas.addEventListener('mousedown', (e) => {
      draggingArp = true;
      state.isDraggingArpOled = true;
      state.isDraggingKnob = true;
      paintArpStep(e);
      e.preventDefault();
    });
    window.addEventListener('mousemove', (e) => {
      if (draggingArp) paintArpStep(e);
    });
    window.addEventListener('mouseup', () => {
      if (draggingArp) {
        draggingArp = false;
        state.isDraggingArpOled = false;
        state.isDraggingKnob = false;
        state.lastEditedArpStep = -1;
        hideParamHudTooltip();
      }
    });
    arpCanvas.addEventListener('dblclick', () => {
      const nextMode = ((state.params.arp_mode || 0) + 1) % 5;
      updateArpModeUI(nextMode);
      sendFastArpMode(nextMode);
    });
  }

  window.__tepuiOnArpSequencerChange = (stepIdx, accent) => {
    const mode = state.params.arp_mode || 0;
    const bars = getArpBarsForMode(mode);
    const s = Math.max(0, Math.min(bars.length - 1, Number(stepIdx) || 0));
    const v = Math.max(0.05, Math.min(1.0, Number(accent) || 0.65));
    bars[s] = v;
    if (mode === 0) {
      [1, 2, 3, 4].forEach((m) => {
        getArpBarsForMode(m)[s] = v;
      });
    }
    state.lastEditedArpStep = s;
    state.currentVelocity = v;
    state.lastPlayedVelocity = v;
    sendIpc('arpstep,' + s + ',' + v.toFixed(3));
    syncArpPatternToDsp(mode);
    oledDirtyCache.arpSig = '';
    drawArpOled();
  };

  // 4. Interactive 2D FX Sculpting on #oled-reverb (X: Spatial Canyon Mix, Y: Drive / Shift+Y: OTT)
  const revCanvas = document.getElementById('oled-reverb');
  if (revCanvas) {
    revCanvas.style.cursor = 'crosshair';
    let draggingRev = false;
    const applyReverb2D = (e) => {
      const rect = revCanvas.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;
      const normX = Math.max(0.0, Math.min(1.0, (e.clientX - rect.left) / rect.width));
      const normY = Math.max(0.0, Math.min(1.0, 1.0 - (e.clientY - rect.top) / rect.height));
      state.normalized.space_mix = normX;
      state.params.space_mix = normX;
      updateKnobUI('space_mix', normX);
      sendFastParam('space_mix', normX);
      if (e.shiftKey) {
        state.normalized.ott = normY;
        state.params.ott = normY;
        updateKnobUI('ott', normY);
        sendFastParam('ott', normY);
        showParamHudTooltip(
          revCanvas,
          'FX 2D (OTT)',
          `SPATIAL ${Math.round(normX * 100)}% • OTT ${Math.round(normY * 100)}%`
        );
      } else {
        state.normalized.drive = normY;
        state.params.drive = normY;
        updateKnobUI('drive', normY);
        sendFastParam('drive', normY);
        showParamHudTooltip(
          revCanvas,
          'FX 2D',
          `SPATIAL ${Math.round(normX * 100)}% • DRIVE ${Math.round(normY * 100)}%`
        );
      }
    };

    revCanvas.addEventListener('mousedown', (e) => {
      draggingRev = true;
      state.isDraggingKnob = true;
      applyReverb2D(e);
      e.preventDefault();
    });
    window.addEventListener('mousemove', (e) => {
      if (draggingRev) applyReverb2D(e);
    });
    window.addEventListener('mouseup', () => {
      if (draggingRev) {
        draggingRev = false;
        state.isDraggingKnob = false;
        hideParamHudTooltip();
      }
    });
    revCanvas.addEventListener('dblclick', () => {
      state.normalized.drive = 0.0;
      state.normalized.ott = 0.0;
      state.normalized.space_mix = 0.15;
      state.params.drive = 0.0;
      state.params.ott = 0.0;
      state.params.space_mix = 0.15;
      updateKnobUI('drive', 0.0);
      updateKnobUI('ott', 0.0);
      updateKnobUI('space_mix', 0.15);
      sendFastParam('drive', 0.0);
      sendFastParam('ott', 0.0);
      sendFastParam('space_mix', 0.15);
    });
  }
}

function initOledThemeSwitcher() {
  let urlSkin = null;
  let savedSkin = null;
  try {
    urlSkin = new URLSearchParams(window.location.search).get('skin');
    savedSkin = localStorage.getItem('tepui_ui_skin');
  } catch (_e) {}

  const statusObj = window.__TEPUI_AUDIO_STATUS__;
  const statusSkin = statusObj && (statusObj.active_ui_skin || (statusObj.config && statusObj.config.ui_skin));
  const initialSkin = urlSkin || savedSkin || statusSkin || 'minimal';
  applyUiSkin(initialSkin, false);

  const btnTheme = document.getElementById('btn-oled-theme');
  if (btnTheme) {
    btnTheme.addEventListener('click', (e) => {
      if (e.shiftKey && typeof window.__tepuiOpenSettingsTab === 'function') {
        window.__tepuiOpenSettingsTab('skin');
        return;
      }
      const idx = UI_SKINS.findIndex((s) => s.id === currentUiSkin);
      const nextSkin = UI_SKINS[(idx + 1) % UI_SKINS.length] || UI_SKINS[0];
      applyUiSkin(nextSkin.id, true);
      if (typeof window.__tepuiPersistSkinConfig === 'function') {
        window.__tepuiPersistSkinConfig(nextSkin.id);
      }
    });
  }

  const skinButtons = [
    { id: 'btn-skin-minimal', skin: 'minimal' },
    { id: 'btn-skin-dark', skin: 'dark' },
    { id: 'btn-skin-retro', skin: 'retro' },
    { id: 'btn-skin-synth70', skin: 'synth70' },
    { id: 'btn-skin-gunmetal', skin: 'gunmetal' },
    { id: 'btn-skin-phosphor', skin: 'phosphor' },
  ];
  skinButtons.forEach(({ id, skin }) => {
    const btn = document.getElementById(id);
    if (!btn) return;
    btn.addEventListener('click', () => {
      applyUiSkin(skin, true);
      if (typeof window.__tepuiPersistSkinConfig === 'function') {
        window.__tepuiPersistSkinConfig(skin);
      }
    });
  });
}

function updateTelemetry(info) {
  if (!info) return;
  if (info.name) {
    state.sampleName = info.name;
  }
  if (info.root_note !== undefined) {
    updateRootNoteDisplay(info.root_note, info.name);
  } else if (info.name) {
    updateRootNoteDisplay(state.params.root_note, info.name);
  }
  if (info.duration_secs !== undefined) {
    const durEl = document.getElementById('telemetry-dur');
    if (durEl) durEl.innerText = `${info.duration_secs.toFixed(2)}s`;
  }
  if (info.num_samples !== undefined) {
    const sampEl = document.getElementById('telemetry-samples');
    if (sampEl) sampEl.innerText = `${info.num_samples.toLocaleString()} smp`;
  }
  if (info.sample_rate !== undefined) {
    const srEl = document.getElementById('telemetry-sr');
    if (srEl) srEl.innerText = `${(info.sample_rate / 1000).toFixed(1)} kHz STEREO`;
  }
}

// -------------------------------------------------------------
// 7. RESPONSIVE PIANO KEYBOARD & QWERTY MUSICAL TYPING
// -------------------------------------------------------------
// Definición semitonal relativa al C base de la octava actual
const WHITE_KEY_DEFS = [
  { semi: 0,  note: 'C',  shortcut: 'A' },
  { semi: 2,  note: 'D',  shortcut: 'S' },
  { semi: 4,  note: 'E',  shortcut: 'D' },
  { semi: 5,  note: 'F',  shortcut: 'F' },
  { semi: 7,  note: 'G',  shortcut: 'G' },
  { semi: 9,  note: 'A',  shortcut: 'H' },
  { semi: 11, note: 'B',  shortcut: 'J' },
  { semi: 12, note: 'C',  shortcut: 'K' },
  { semi: 14, note: 'D',  shortcut: 'L' },
  { semi: 16, note: 'E',  shortcut: ';' },
  { semi: 17, note: 'F',  shortcut: "'" },
  { semi: 19, note: 'G',  shortcut: '' },
  { semi: 21, note: 'A',  shortcut: '' },
  { semi: 23, note: 'B',  shortcut: '' },
  { semi: 24, note: 'C',  shortcut: '' }
];

const BLACK_KEY_DEFS = [
  { semi: 1,  afterIndex: 0,  note: 'C#', shortcut: 'W' },
  { semi: 3,  afterIndex: 1,  note: 'D#', shortcut: 'E' },
  { semi: 6,  afterIndex: 3,  note: 'F#', shortcut: 'T' },
  { semi: 8,  afterIndex: 4,  note: 'G#', shortcut: 'Y' },
  { semi: 10, afterIndex: 5,  note: 'A#', shortcut: 'U' },
  { semi: 13, afterIndex: 7,  note: 'C#', shortcut: 'O' },
  { semi: 15, afterIndex: 8,  note: 'D#', shortcut: 'P' },
  { semi: 18, afterIndex: 10, note: 'F#', shortcut: '[' },
  { semi: 20, afterIndex: 11, note: 'G#', shortcut: ']' },
  { semi: 22, afterIndex: 12, note: 'A#', shortcut: '' }
];

// Mapa de teclas físicas del teclado (inmune a CapsLock / Shift / Layouts internacionales)
const CODE_TO_SEMI = {
  // Octava 1 (relativa al C base de la octava seleccionada)
  'KeyA': 0,        // C
  'KeyW': 1,        // C#
  'KeyS': 2,        // D
  'KeyE': 3,        // D#
  'KeyD': 4,        // E
  'KeyF': 5,        // F
  'KeyT': 6,        // F#
  'KeyG': 7,        // G
  'KeyY': 8,        // G#
  'KeyH': 9,        // A
  'KeyU': 10,       // A#
  'KeyJ': 11,       // B
  // Octava 2
  'KeyK': 12,       // C
  'KeyO': 13,       // C#
  'KeyL': 14,       // D
  'KeyP': 15,       // D#
  'Semicolon': 16,  // E
  'Quote': 17,      // F
  'BracketLeft': 18,// F#
  'BracketRight': 20// G#
};

// Fallback por nombre de tecla (soporta teclados en español y variantes)
const KEY_FALLBACK_MAP = {
  'a': 0, 'w': 1, 's': 2, 'e': 3, 'd': 4, 'f': 5,
  't': 6, 'g': 7, 'y': 8, 'h': 9, 'u': 10, 'j': 11,
  'k': 12, 'o': 13, 'l': 14, 'p': 15, ';': 16, "'": 17,
  '[': 18, ']': 20, 'ñ': 16
};

let activeMouseNote = null;
let isPointerDown = false;
const activePhysicalKeys = new Map(); // trackingId (e.code o e.key) -> midiNote

function getBaseMidiNote() {
  return (state.currentOctave + 1) * 12; // Octava 3 -> 48 (C3)
}

function initKeyboard() {
  synthStore.setCurrentOctave(state.currentOctave);
  const octBadge = document.getElementById('octave-badge');
  if (octBadge) {
    octBadge.innerText = `OCT ${state.currentOctave} (C${state.currentOctave} - C${state.currentOctave + 2})`;
  }
}

function initKeyboardEvents() {
  const piano = document.getElementById('piano-keyboard');
  if (!piano) return;

  // Soporte Pointer Events de alto rendimiento (ratón y multitáctil con captura)
  piano.addEventListener('pointerdown', (e) => {
    isPointerDown = true;
    try { piano.setPointerCapture(e.pointerId); } catch (_) {}
    handlePointerNote(e);
  });

  piano.addEventListener('pointermove', (e) => {
    if (isPointerDown) handlePointerNote(e);
  });

  const stopPointer = (e) => {
    if (isPointerDown) {
      isPointerDown = false;
      try { piano.releasePointerCapture(e.pointerId); } catch (_) {}
      if (activeMouseNote !== null) {
        releaseNote(activeMouseNote);
        activeMouseNote = null;
      }
    }
  };

  piano.addEventListener('pointerup', stopPointer);
  piano.addEventListener('pointercancel', stopPointer);
  window.addEventListener('pointerup', stopPointer);

  // TECLADO DE COMPUTADORA (QWERTY) CON DETECCIÓN CERO-LAG
  window.addEventListener('keydown', (e) => {
    if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;
    if (e.repeat) return; // PREVIENE REPETICIONES DE TECLAS DEL OS

    const code = e.code;
    const k = e.key ? e.key.toLowerCase() : '';

    // Atajos de cambio de octava
    if (code === 'KeyZ' || k === 'z') {
      shiftOctave(-1);
      e.preventDefault();
      return;
    }
    if (code === 'KeyX' || k === 'x') {
      shiftOctave(1);
      e.preventDefault();
      return;
    }

    // Atajos de velocidad
    if (code === 'KeyC' || k === 'c') {
      shiftVelocity(-0.1);
      e.preventDefault();
      return;
    }
    if (code === 'KeyV' || k === 'v') {
      shiftVelocity(0.1);
      e.preventDefault();
      return;
    }

    // Pánico / Kill all
    if (code === 'Escape' || k === 'escape') {
      panicAllNotes();
      e.preventDefault();
      return;
    }

    // Notas musicales QWERTY prioritarias por código físico de tecla
    let semi = CODE_TO_SEMI[code];
    if (semi === undefined) {
      semi = KEY_FALLBACK_MAP[k];
    }

    if (semi !== undefined) {
      e.preventDefault();
      const trackingId = code || k;
      if (activePhysicalKeys.has(trackingId)) return;

      const midi = getBaseMidiNote() + semi;
      if (midi >= 0 && midi <= 127) {
        activePhysicalKeys.set(trackingId, midi);
        pressNote(midi, state.currentVelocity);
      }
    }
  });

  window.addEventListener('keyup', (e) => {
    if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;
    const code = e.code;
    const k = e.key ? e.key.toLowerCase() : '';

    let trackingId = null;
    if (code && activePhysicalKeys.has(code)) {
      trackingId = code;
    } else if (k && activePhysicalKeys.has(k)) {
      trackingId = k;
    }

    if (trackingId !== null) {
      const midi = activePhysicalKeys.get(trackingId);
      activePhysicalKeys.delete(trackingId);
      releaseNote(midi);
    }
  });

  // Si la ventana pierde foco (ej. clic a otra pista en el DAW o Alt-Tab), cortar notas
  window.addEventListener('blur', () => {
    panicAllNotes();
  });

  // Botones de la barra de teclado
  document.getElementById('btn-oct-down').addEventListener('click', () => shiftOctave(-1));
  document.getElementById('btn-oct-up').addEventListener('click', () => shiftOctave(1));
  document.getElementById('btn-vel-down').addEventListener('click', () => shiftVelocity(-0.1));
  document.getElementById('btn-vel-up').addEventListener('click', () => shiftVelocity(0.1));
  document.getElementById('btn-panic').addEventListener('click', panicAllNotes);

  // Pedal Sustain CC#64 (Botón SUS)
  const susBtn = document.getElementById('btn-sustain-pedal');
  if (susBtn) {
    susBtn.addEventListener('click', () => {
      state.sustainPedal = !state.sustainPedal;
      syncExpressionWheelsUI();
      sendIpc(`sus,${state.sustainPedal ? 1 : 0}`);
      refreshFooterOddText();
    });
  }

  // Botón Glide / Portamento Exponencial y Modo Legato
  const glideBtn = document.getElementById('btn-glide-legato');
  if (glideBtn) {
    const GLIDE_MODES = [
      { modeIdx: 0, time: 0.000, legato: false },
      { modeIdx: 1, time: 0.085, legato: false },
      { modeIdx: 2, time: 0.140, legato: true }
    ];
    let glideStep = 0;
    glideBtn.addEventListener('click', () => {
      glideStep = (glideStep + 1) % GLIDE_MODES.length;
      const g = GLIDE_MODES[glideStep];
      updateGlideUI(g.time, g.legato);
      sendIpc(`glide,${g.modeIdx}`);
      refreshFooterOddText();
    });
  }

  // Botón MIDI Learn directo en la barra de teclado
  const midiLearnBtn = document.getElementById('btn-midi-learn');
  if (midiLearnBtn) {
    const LEARN_CYCLE_PARAMS = ['cutoff', 'resonance', 'wt_pos', 'wt_warp', 'drive', 'ott', 'macro1', null];
    let learnIdx = -1;
    midiLearnBtn.addEventListener('click', () => {
      learnIdx = (learnIdx + 1) % LEARN_CYCLE_PARAMS.length;
      const targetParam = LEARN_CYCLE_PARAMS[learnIdx];
      state.midiLearnArmedParam = targetParam;
      if (targetParam) {
        sendIpc(`midi_learn:${targetParam}`);
        showParamHudTooltip(midiLearnBtn, `MIDI LEARN → ${targetParam.toUpperCase()}`, 'CLIC DERECHO EN CUALQUIER PERILLA O MUEVE CC');
        setTimeout(hideParamHudTooltip, 1300);
      } else {
        sendIpc('midi_learn:off');
      }
      syncMidiLearnUI();
    });
  }

  // Arrastre vertical y rueda de ratón en #vel-badge para ajuste continuo de Velocity
  const velBadge = document.getElementById('vel-badge');
  if (velBadge) {
    let draggingVel = false;
    let startY = 0;
    let startVel = 0.95;
    velBadge.addEventListener('pointerdown', (e) => {
      draggingVel = true;
      startY = e.clientY;
      startVel = state.currentVelocity || 0.95;
      try { velBadge.setPointerCapture(e.pointerId); } catch (_) {}
      e.preventDefault();
    });
    velBadge.addEventListener('pointermove', (e) => {
      if (!draggingVel) return;
      const dy = startY - e.clientY;
      const nextVel = Math.max(0.05, Math.min(1.0, startVel + dy * 0.008));
      state.currentVelocity = nextVel;
      state.lastPlayedVelocity = nextVel;
      sendIpc(`vel,${nextVel.toFixed(3)}`);
      velBadge.innerText = `${Math.round(nextVel * 100)}%`;
      refreshFooterOddText();
    });
    const endVelDrag = (e) => {
      if (!draggingVel) return;
      draggingVel = false;
      try { velBadge.releasePointerCapture(e.pointerId); } catch (_) {}
    };
    velBadge.addEventListener('pointerup', endVelDrag);
    velBadge.addEventListener('pointercancel', endVelDrag);
    velBadge.addEventListener('wheel', (e) => {
      e.preventDefault();
      shiftVelocity(e.deltaY < 0 ? 0.05 : -0.05);
    }, { passive: false });
  }

  // Rueda interactiva PITCH BEND (-2.0 .. +2.0 semitonos con retorno por resorte a 0.0)
  const pitchWheel = document.getElementById('wheel-pitch');
  if (pitchWheel) {
    let draggingPitch = false;
    const applyPitchFromEvent = (e) => {
      const rect = pitchWheel.getBoundingClientRect();
      if (rect.height <= 0) return;
      const relY = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));
      const semitones = (0.5 - relY) * 4.0; // Top = +2.0, Center = 0.0, Bottom = -2.0
      state.pitchBendSemitones = Math.max(-2.0, Math.min(2.0, semitones));
      syncExpressionWheelsUI();
      sendIpc(`pb,${state.pitchBendSemitones.toFixed(3)}`);
    };
    pitchWheel.addEventListener('pointerdown', (e) => {
      draggingPitch = true;
      state.isDraggingWheel = true;
      pitchWheel.classList.add('dragging');
      try { pitchWheel.setPointerCapture(e.pointerId); } catch (_) {}
      applyPitchFromEvent(e);
      e.preventDefault();
    });
    pitchWheel.addEventListener('pointermove', (e) => {
      if (draggingPitch) applyPitchFromEvent(e);
    });
    const releasePitchWheel = (e) => {
      if (!draggingPitch) return;
      draggingPitch = false;
      state.isDraggingWheel = false;
      pitchWheel.classList.remove('dragging');
      try { pitchWheel.releasePointerCapture(e.pointerId); } catch (_) {}
      // Retorno automático por resorte a 0.0 semitonos
      state.pitchBendSemitones = 0.0;
      syncExpressionWheelsUI();
      sendIpc('pb,0.000');
    };
    pitchWheel.addEventListener('pointerup', releasePitchWheel);
    pitchWheel.addEventListener('pointercancel', releasePitchWheel);
  }

  // Rueda interactiva MODULATION CC#1 (0.0 .. 1.0, doble clic resetea a 0)
  const modWheel = document.getElementById('wheel-mod');
  if (modWheel) {
    let draggingMod = false;
    const applyModFromEvent = (e) => {
      const rect = modWheel.getBoundingClientRect();
      if (rect.height <= 0) return;
      const relY = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));
      const norm = Math.max(0.0, Math.min(1.0, 1.0 - relY));
      state.modWheelNorm = norm;
      syncExpressionWheelsUI();
      sendIpc(`mw,${norm.toFixed(3)}`);
    };
    modWheel.addEventListener('pointerdown', (e) => {
      draggingMod = true;
      state.isDraggingWheel = true;
      modWheel.classList.add('dragging');
      try { modWheel.setPointerCapture(e.pointerId); } catch (_) {}
      applyModFromEvent(e);
      e.preventDefault();
    });
    modWheel.addEventListener('pointermove', (e) => {
      if (draggingMod) applyModFromEvent(e);
    });
    const releaseModWheel = (e) => {
      if (!draggingMod) return;
      draggingMod = false;
      state.isDraggingWheel = false;
      modWheel.classList.remove('dragging');
      try { modWheel.releasePointerCapture(e.pointerId); } catch (_) {}
    };
    modWheel.addEventListener('pointerup', releaseModWheel);
    modWheel.addEventListener('pointercancel', releaseModWheel);
    modWheel.addEventListener('dblclick', () => {
      state.modWheelNorm = 0.0;
      syncExpressionWheelsUI();
      sendIpc('mw,0.000');
    });
  }

  const arpBtn = document.getElementById('btn-arp-mode');
  if (arpBtn) {
    arpBtn.addEventListener('click', () => {
      const nextMode = ((state.params.arp_mode || 0) + 1) % 5;
      state.params.arp_mode = nextMode;
      updateArpModeUI(nextMode);
      sendFastArpMode(nextMode);
    });
  }

  const recBtn = document.getElementById('btn-live-rec');
  if (recBtn) {
    recBtn.addEventListener('click', async () => {
      const isRec = !!(state.oddMetrics && state.oddMetrics.is_recording_master);
      if (!isRec) {
        if (!state.oddMetrics) state.oddMetrics = {};
        state.oddMetrics.is_recording_master = true;
        recBtn.classList.add('active', 'panic-btn');
        recBtn.innerText = '■ STOP REC';
        sendIpc('rec,start');
      } else {
        state.oddMetrics.is_recording_master = false;
        recBtn.classList.remove('active', 'panic-btn');
        recBtn.innerText = '● REC LIVE';
        sendIpc('rec,stop');
        try {
          const resp = await fetch('/api/export-live-rec');
          if (resp.ok) {
            const blob = await resp.blob();
            const url = URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = 'TEPUI_Live_Master_Session.wav';
            document.body.appendChild(a);
            a.click();
            document.body.removeChild(a);
            setTimeout(() => URL.revokeObjectURL(url), 2500);
          }
        } catch (err) {
          console.warn('[TEPUI LIVE REC EXPORT ERROR]', err);
        }
      }
    });
  }
}

const ARP_LABELS = [
  'ARP: OFF',
  'ARP: UP',
  'ARP: DOWN',
  '6/8 SYNC',
  '5/8 SYNC'
];

function updateArpModeUI(mode) {
  const m = Math.max(0, Math.min(4, Math.round(mode || 0)));
  state.params.arp_mode = m;
  const arpBtn = document.getElementById('btn-arp-mode');
  if (arpBtn) {
    arpBtn.innerText = ARP_LABELS[m] || 'ARP: OFF';
    arpBtn.classList.toggle('active', m > 0);
  }
  document.querySelectorAll('.arp-mode-btn').forEach((b) => {
    const bm = parseInt(b.dataset.arp, 10);
    b.classList.toggle('active', bm === m);
  });
  const meterLbl = document.getElementById('arp-meter-label');
  if (meterLbl) {
    const meters = ['4/4 FREE', '4/4 UP', '4/4 DOWN', '6/8 SYNC', '5/8 SYNC'];
    meterLbl.innerText = meters[m] || '4/4 FREE';
  }
}

function handlePointerNote(e) {
  const target = document.elementFromPoint(e.clientX, e.clientY);
  if (!target) return;
  const keyEl = target.closest('.piano-key');

  if (keyEl && keyEl.dataset.midi) {
    const note = parseInt(keyEl.dataset.midi, 10);
    // Calcular velocidad y expresión MPE Poly Aftertouch / Per-Note Pitch Bend según posición en la tecla
    const rect = keyEl.getBoundingClientRect();
    const relY = rect.height > 0
      ? Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height))
      : 0.9;
    const relX = rect.width > 0
      ? Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width))
      : 0.5;
    const vel = Math.max(0.15, Math.min(1.0, 0.15 + relY * 0.85));

    if (note !== activeMouseNote) {
      if (activeMouseNote !== null && !state.params.glide_legato) {
        releaseNote(activeMouseNote);
      }
      const prevNote = activeMouseNote;
      activeMouseNote = note;
      pressNote(note, vel);
      if (prevNote !== null && state.params.glide_legato) {
        releaseNote(prevNote);
      }
    } else if (e.shiftKey || Math.abs(relX - 0.5) > 0.18) {
      // MPE Expression en vivo al deslizar dentro de la tecla activa: Aftertouch vertical + Per-Note Bend horizontal
      const noteBend = (relX - 0.5) * 2.0;
      sendIpc(`pat,${note},${vel.toFixed(3)}`);
      sendIpc(`npb,${note},${noteBend.toFixed(3)}`);
    }
  } else if (activeMouseNote !== null) {
    releaseNote(activeMouseNote);
    activeMouseNote = null;
  }
}

function pressNote(midi, velocity = 0.95) {
  const el = document.querySelector(`.piano-key[data-midi="${midi}"]`);
  if (el) el.classList.add('active');
  sendFastNoteOn(midi, velocity);
  synthStore.setActiveNotes(state.activeNotes);
}
window.__tepuiPressNote = pressNote;

function releaseNote(midi) {
  const el = document.querySelector(`.piano-key[data-midi="${midi}"]`);
  if (el) el.classList.remove('active');
  sendFastNoteOff(midi);
  synthStore.setActiveNotes(state.activeNotes);
}
window.__tepuiReleaseNote = releaseNote;

function shiftOctave(delta) {
  // Silenciar cualquier nota activa para evitar voces colgadas
  panicAllNotes();

  const newOct = Math.max(1, Math.min(6, state.currentOctave + delta));
  if (newOct !== state.currentOctave) {
    state.currentOctave = newOct;
    initKeyboard();
  }
}

function shiftVelocity(delta) {
  const newVel = Math.max(0.05, Math.min(1.0, state.currentVelocity + delta));
  state.currentVelocity = newVel;
  state.lastPlayedVelocity = newVel;
  sendIpc(`vel,${newVel.toFixed(3)}`);
  const badge = document.getElementById('vel-badge');
  if (badge) badge.innerText = `${Math.round(newVel * 100)}%`;
  refreshFooterOddText();
}

function panicAllNotes() {
  activePhysicalKeys.clear();
  activeMouseNote = null;
  document.querySelectorAll('.piano-key.active').forEach(k => k.classList.remove('active'));

  sendIpc('k');

  // Enviar note off para las notas activas registradas
  for (const note of Array.from(state.activeNotes)) {
    sendFastNoteOff(note);
  }
  state.activeNotes.clear();
  synthStore.setActiveNotes(state.activeNotes);
  updateVoiceCount();
}

// -------------------------------------------------------------
// 8. AI GENERATION HUB, PRESET CHIPS, WAV IMPORT/EXPORT & ROOT NOTE
// -------------------------------------------------------------
async function uploadWavBytes(arrayBuffer, rawFileName, targetZone = null) {
  try {
    const cleanName = (rawFileName || 'Imported WAV').replace(/\.wav$/i, '').trim() || 'Imported WAV';
    const zoneQuery = (targetZone === 0 || targetZone === 1 || targetZone === 2)
      ? `&zone=${targetZone}`
      : '';
    const resp = await fetch(`/api/load-wav?name=${encodeURIComponent(cleanName)}${zoneQuery}`, {
      method: 'POST',
      headers: { 'Content-Type': 'audio/wav' },
      body: arrayBuffer
    });
    if (resp.ok) {
      const data = await resp.json();
      synthStore.setActiveChipId('');
      synthStore.setActivePadId('');
      document.querySelectorAll('.chip').forEach(c => c.classList.remove('active'));
      const promptInput = document.getElementById('prompt-input');
      if (promptInput) promptInput.value = cleanName;
      if (targetZone !== null) {
        state.localWavZoneDrops = (state.localWavZoneDrops || 0) + 1;
      }
      applyInitPayload(data);
    } else {
      const errText = await resp.text();
      console.warn('[WAV Load Error]', errText);
    }
  } catch (e) {
    console.warn('[WAV Upload Exception]', e);
  }
}

async function uploadPatchBytes(arrayBuffer) {
  try {
    const resp = await fetch('/api/load-patch', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: arrayBuffer
    });
    if (resp.ok) {
      const data = await resp.json();
      synthStore.setActiveChipId('');
      synthStore.setActivePadId('');
      document.querySelectorAll('.chip').forEach(c => c.classList.remove('active'));
      if (data.odd_metrics && data.odd_metrics.active_prompt) {
        const promptInput = document.getElementById('prompt-input');
        if (promptInput) promptInput.value = data.odd_metrics.active_prompt;
      }
      applyInitPayload(data);
    } else {
      const errText = await resp.text();
      console.warn('[TEPUI Patch Load Error]', errText);
    }
  } catch (e) {
    console.warn('[TEPUI Patch Upload Exception]', e);
  }
}

function initWavImportExport() {
  const btnImport = document.getElementById('btn-import-wav');
  const btnExport = document.getElementById('btn-export-wav');
  const btnLoadPatch = document.getElementById('btn-load-patch');
  const btnSavePatch = document.getElementById('btn-save-patch');
  const fileInput = document.getElementById('wav-file-input');
  const patchInput = document.getElementById('patch-file-input');
  const screenBody = document.querySelector('.screen-body');

  if (btnImport && fileInput) {
    btnImport.addEventListener('click', () => {
      fileInput.value = '';
      fileInput.click();
    });
    fileInput.addEventListener('change', async (e) => {
      const file = e.target.files && e.target.files[0];
      if (!file) return;
      const buf = await file.arrayBuffer();
      await uploadWavBytes(buf, file.name);
    });
  }

  if (btnExport) {
    btnExport.addEventListener('click', async () => {
      try {
        const resp = await fetch('/api/export-wav');
        if (!resp.ok) return;
        const blob = await resp.blob();
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        const safeName = (state.sampleName || 'tepui_sample').toLowerCase().replace(/[^a-z0-9_-]+/g, '_');
        a.href = url;
        a.download = `${safeName}.wav`;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        setTimeout(() => URL.revokeObjectURL(url), 1500);
      } catch (e) {
        console.warn('[WAV Export Error]', e);
      }
    });
  }

  if (btnLoadPatch && patchInput) {
    btnLoadPatch.addEventListener('click', () => {
      patchInput.value = '';
      patchInput.click();
    });
    patchInput.addEventListener('change', async (e) => {
      const file = e.target.files && e.target.files[0];
      if (!file) return;
      const buf = await file.arrayBuffer();
      await uploadPatchBytes(buf);
    });
  }

  if (btnSavePatch) {
    btnSavePatch.addEventListener('click', async () => {
      try {
        const resp = await fetch('/api/export-patch');
        if (!resp.ok) return;
        const blob = await resp.blob();
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        const safeName = (state.sampleName || 'tepui_patch').toLowerCase().replace(/[^a-z0-9_-]+/g, '_');
        a.href = url;
        a.download = `${safeName}.tepui`;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        setTimeout(() => URL.revokeObjectURL(url), 1500);
      } catch (e) {
        console.warn('[TEPUI Patch Export Error]', e);
      }
    });
  }

  if (screenBody) {
    screenBody.addEventListener('dragover', (e) => {
      e.preventDefault();
      screenBody.classList.add('drag-over');
    });
    screenBody.addEventListener('dragleave', () => {
      screenBody.classList.remove('drag-over');
    });
    screenBody.addEventListener('drop', async (e) => {
      e.preventDefault();
      screenBody.classList.remove('drag-over');
      const file = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
      if (!file) return;
      const buf = await file.arrayBuffer();
      if (file.name && (file.name.toLowerCase().endsWith('.tepui') || file.name.toLowerCase().endsWith('.json'))) {
        await uploadPatchBytes(buf);
      } else {
        await uploadWavBytes(buf, file.name);
      }
    });
  }
}

function initLfoAndKeyzoneControls() {
  const btnLfoTarget = document.getElementById('btn-lfo-target');
  if (btnLfoTarget) {
    btnLfoTarget.addEventListener('click', () => {
      const nextTarget = ((state.params.lfo_target || 0) + 1) % 4;
      updateLfoTargetUI(nextTarget);
      sendIpc(`lt,${nextTarget}`);
    });
  }

  const btnLfoShape = document.getElementById('btn-lfo-shape');
  if (btnLfoShape) {
    btnLfoShape.addEventListener('click', () => {
      const nextShape = ((state.params.lfo_shape || 0) + 1) % 6;
      updateLfoShapeUI(nextShape);
      sendIpc(`ls,${nextShape}`);
    });
  }

  const zLow = document.getElementById('btn-zone-low');
  const zMid = document.getElementById('btn-zone-mid');
  const zHigh = document.getElementById('btn-zone-high');
  const zClr = document.getElementById('btn-zone-clear');

  if (zLow) zLow.addEventListener('click', () => sendIpc('z,0'));
  if (zMid) zMid.addEventListener('click', () => sendIpc('z,1'));
  if (zHigh) zHigh.addEventListener('click', () => sendIpc('z,2'));
  if (zClr) zClr.addEventListener('click', () => sendIpc('z,clear'));

  // Drag & Drop directo de archivos .wav sobre Z-LOW (0), Z-MID (1) y Z-HIGH (2)
  [
    { el: zLow, zone: 0 },
    { el: zMid, zone: 1 },
    { el: zHigh, zone: 2 }
  ].forEach(({ el, zone }) => {
    if (!el) return;
    el.addEventListener('dragover', (e) => {
      e.preventDefault();
      e.stopPropagation();
      el.classList.add('drag-over');
    });
    el.addEventListener('dragleave', () => {
      el.classList.remove('drag-over');
    });
    el.addEventListener('drop', async (e) => {
      e.preventDefault();
      e.stopPropagation();
      el.classList.remove('drag-over');
      const file = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
      if (!file) return;
      const buf = await file.arrayBuffer();
      await uploadWavBytes(buf, file.name, zone);
    });
  });

  // Interactive 5x5 LFO & Mod Matrix Pins with ODD Telemetry
  const MATRIX_ROW_PROFILES = [
    { name: 'CUATRO', target: 0, rateNorm: 0.42, baseDepth: 0.35 },
    { name: 'ARPA', target: 1, rateNorm: 0.52, baseDepth: 0.18 },
    { name: 'BANDOLA', target: 0, rateNorm: 0.34, baseDepth: 0.55 },
    { name: 'CUMACO', target: 2, rateNorm: 0.60, baseDepth: 0.48 },
    { name: 'FURRUCO', target: 0, rateNorm: 0.18, baseDepth: 0.65 }
  ];

  document.querySelectorAll('.mod-matrix-mini .matrix-row').forEach((rowEl, rowIdx) => {
    const pins = rowEl.querySelectorAll('i');
    const applyMatrixPin = (colIdx, forceSingleRow = false) => {
      const bit = 1 << (rowIdx * 5 + colIdx);
      let nextMask = (state.params.mod_matrix_mask || 0) & 0x01ffffff;
      if (forceSingleRow) {
        const rowClear = ~(0b11111 << (rowIdx * 5));
        nextMask = (nextMask & rowClear) | bit;
      } else {
        nextMask ^= bit;
      }
      const isNowOn = (nextMask & bit) !== 0;
      syncMatrixPinsFromMask(nextMask);

      const prof = MATRIX_ROW_PROFILES[rowIdx] || MATRIX_ROW_PROFILES[0];
      const depthScale = 0.35 + (colIdx / 4) * 0.65;
      const newDepth = Math.max(0.05, Math.min(1.0, prof.baseDepth * depthScale));
      const newRate = Math.max(0.05, Math.min(1.0, prof.rateNorm + (colIdx - 2) * 0.06));

      state.normalized.lfo_rate = newRate;
      state.normalized.lfo_depth = newDepth;
      state.params.lfo_depth = newDepth;
      updateKnobUI('lfo_rate', newRate);
      updateKnobUI('lfo_depth', newDepth);
      updateLfoTargetUI(prof.target);

      sendFastParam('lfo_rate', newRate);
      sendFastParam('lfo_depth', newDepth);
      sendIpc(`lt,${prof.target}`);

      state.localUiMatrixSwitches = (state.localUiMatrixSwitches || 0) + 1;
      sendIpc(`matrix:${rowIdx},${colIdx},${isNowOn ? 1 : 0}`);
      refreshFooterOddText();
    };

    const SRC_NAMES = ['LFO 1', 'LFO 2', 'ENV 2', 'VEL', 'MOD W'];
    const DST_NAMES = ['CUTOFF', 'RESONANCE', 'WAVETABLE POS', 'PITCH / VIBRATO', 'OTT / FX MIX'];
    pins.forEach((pinEl, colIdx) => {
      pinEl.title = `${SRC_NAMES[rowIdx] || 'SRC'} → ${DST_NAMES[colIdx] || 'DST'} (Clic para activar/desactivar ruta)`;
      pinEl.addEventListener('click', () => applyMatrixPin(colIdx, false));
    });
    const lbl = rowEl.querySelector('.m-lbl');
    if (lbl) {
      lbl.addEventListener('click', () => applyMatrixPin(rowIdx % 5, true));
    }
  });
}

function initRootNoteControls() {
  const btnDown = document.getElementById('btn-root-down');
  const btnUp = document.getElementById('btn-root-up');
  if (btnDown) {
    btnDown.addEventListener('click', () => {
      sendFastRootNote((state.params.root_note ?? 60) - 1);
    });
  }
  if (btnUp) {
    btnUp.addEventListener('click', () => {
      sendFastRootNote((state.params.root_note ?? 60) + 1);
    });
  }
}

function capturePatchSnapshot() {
  const input = document.getElementById('prompt-input');
  return {
    prompt: input ? input.value : 'Concert Grand Piano C4',
    sampleName: state.sampleName,
    normalized: { ...state.normalized },
    params: { ...state.params },
    waveform: Array.isArray(state.waveform) ? [...state.waveform] : []
  };
}

function saveSnapshotBeforeChange() {
  if (!state.abSnapshots) {
    state.abSnapshots = { A: capturePatchSnapshot(), B: capturePatchSnapshot() };
    state.activeAbSlot = 'A';
  }
  const currentSnap = capturePatchSnapshot();
  const inactiveSlot = state.activeAbSlot === 'A' ? 'B' : 'A';
  state.abSnapshots[inactiveSlot] = currentSnap;
}

function restorePatchSnapshot(snap) {
  if (!snap) return;
  const input = document.getElementById('prompt-input');
  if (input && snap.prompt) input.value = snap.prompt;
  if (snap.sampleName) state.sampleName = snap.sampleName;
  if (snap.params) Object.assign(state.params, snap.params);
  if (snap.normalized) {
    Object.assign(state.normalized, snap.normalized);
    for (const [k, v] of Object.entries(snap.normalized)) {
      updateKnobUI(k, v);
      sendFastParam(k, v);
    }
  }
  if (Array.isArray(snap.waveform) && snap.waveform.length > 0) {
    state.waveform = [...snap.waveform];
  }
  if (state.params.filter_mode !== undefined) {
    updateFilterModeUI(state.params.filter_mode);
    sendIpc(`m,${state.params.filter_mode}`);
  }
  if (state.params.lfo_target !== undefined) {
    updateLfoTargetUI(state.params.lfo_target);
    sendIpc(`lt,${state.params.lfo_target}`);
  }
  if (state.params.lfo_shape !== undefined) {
    updateLfoShapeUI(state.params.lfo_shape);
    sendIpc(`ls,${state.params.lfo_shape}`);
  }
  if (state.params.unison_voices !== undefined) {
    updateUnisonVoicesUI(state.params.unison_voices);
    sendIpc(`uni,${state.params.unison_voices}`);
  }
  if (state.params.fm_from_b !== undefined) {
    updateFmFromBUI(state.params.fm_from_b);
    sendIpc(`fmb,${Number(state.params.fm_from_b).toFixed(2)}`);
  }
  if (state.params.arp_mode !== undefined) {
    updateArpModeUI(state.params.arp_mode);
    sendIpc(`arp,${state.params.arp_mode}`);
  }
  if (state.params.mod_matrix_mask !== undefined) {
    syncMatrixPinsFromMask(state.params.mod_matrix_mask);
  }
  if (state.params.root_note !== undefined) {
    sendFastRootNote(state.params.root_note);
    updateRootNoteDisplay(state.params.root_note, state.sampleName);
  }
  if (snap.prompt) {
    sendFastGenerate(snap.prompt);
  }
  refreshFooterOddText();
}

function initAbCompareButton() {
  const btnAb = document.getElementById('btn-ab-compare');
  const slotA = document.getElementById('ab-slot-a');
  const slotB = document.getElementById('ab-slot-b');
  if (!btnAb) return;

  const snapA = capturePatchSnapshot();
  const snapB = {
    prompt: 'Hyper Supersaw Anthem Lead C4',
    sampleName: 'HYPER SUPERSAW ANTHEM • C4',
    normalized: {
      ...snapA.normalized,
      attack: 0.005,
      decay: 0.28,
      sustain: 0.85,
      release: 0.34,
      cutoff: 0.92,
      resonance: 0.18,
      space_mix: 0.38,
      wt_pos: 0.48,
      wt_warp: 0.22,
      unison_detune: 0.58,
      sub_level: 0.25,
      drive: 0.22,
      ott: 0.48
    },
    params: {
      ...snapA.params,
      unison_voices: 7,
      arp_mode: 1,
      root_note: 60
    },
    waveform: snapA.waveform
  };
  state.abSnapshots = { A: snapA, B: snapB };
  state.activeAbSlot = 'A';

  btnAb.addEventListener('click', () => {
    const curSlot = state.activeAbSlot || 'A';
    const nextSlot = curSlot === 'A' ? 'B' : 'A';
    state.abSnapshots[curSlot] = capturePatchSnapshot();
    state.activeAbSlot = nextSlot;

    if (slotA) slotA.classList.toggle('active', nextSlot === 'A');
    if (slotB) slotB.classList.toggle('active', nextSlot === 'B');
    btnAb.classList.toggle('slot-b-active', nextSlot === 'B');

    restorePatchSnapshot(state.abSnapshots[nextSlot]);
    showParamHudTooltip(btnAb, `SLOT ${nextSlot}`, state.abSnapshots[nextSlot]?.prompt || 'PATCH');
    setTimeout(hideParamHudTooltip, 900);
  });
}

function initAiHub() {
  const btn = document.getElementById('btn-generate');
  const input = document.getElementById('prompt-input');

  initAbCompareButton();

  btn.addEventListener('click', () => {
    if (state.isGenerating) return;
    saveSnapshotBeforeChange();
    const prompt = input.value.trim() || 'Concert Grand Piano C4';
    state.isGenerating = true;
    updateGeneratingUI(true);
    sendFastGenerate(prompt);
    // Persistir también como preset de usuario en el catálogo y en disco
    const customId = `usr-${Date.now()}`;
    const userPreset = {
      id: customId,
      name: prompt.slice(0, 28),
      pack: 'pack-03',
      cat: 'Leads',
      tags: 'AI Custom, User',
      rhythm: '4/4',
      arp: state.params.arp_mode || 0,
      prompt,
      root: state.params.root_note || 60,
      matrixMask: state.params.mod_matrix_mask || 2178,
      norm: { ...state.normalized }
    };
    PRESET_CATALOG.unshift(userPreset);
    favoritePresets.add(customId);
    state.localUserPresetsSaved = (state.localUserPresetsSaved || 0) + 1;
    sendIpc(`user_preset_save:${JSON.stringify(userPreset)}`);
    refreshFooterOddText();
  });

  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      btn.click();
      input.blur(); // Regresar el foco al instrumento para tocar inmediatamente
    }
  });

  document.querySelectorAll('.chip').forEach((chip) => {
    chip.addEventListener('click', (e) => {
      saveSnapshotBeforeChange();
      if (chip.classList.contains('folklore-pad')) {
        synthStore.setActivePadId(chip.id);
        synthStore.setActiveChipId('');
      } else {
        synthStore.setActiveChipId(chip.id);
        synthStore.setActivePadId('');
      }
      document.querySelectorAll('.chip').forEach(c => c.classList.remove('active'));
      chip.classList.add('active');
      const rect = chip.getBoundingClientRect();
      const relY = (rect && rect.height > 0 && typeof e.clientY === 'number')
        ? Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height))
        : 0.9;
      const padVel = Math.max(0.25, Math.min(1.0, 0.25 + relY * 0.75));
      state.currentVelocity = padVel;
      state.lastPlayedVelocity = padVel;
      const velBadge = document.getElementById('vel-badge');
      if (velBadge) velBadge.innerText = `${Math.round(padVel * 100)}%`;

      if (chip.dataset.arp !== undefined) {
        const arpM = parseInt(chip.dataset.arp, 10);
        if (!Number.isNaN(arpM)) {
          updateArpModeUI(arpM);
          sendIpc(`arp,${arpM}`);
        }
      }

      // Aplicar perfil Vital/Serum (Wavetable, Warp, Unison, Sub-Osc, Drive, OTT) del pad seleccionado
      const vitalPadMap = [
        ['wtPos', 'wt_pos'],
        ['wtWarp', 'wt_warp'],
        ['detune', 'unison_detune'],
        ['sub', 'sub_level'],
        ['drive', 'drive'],
        ['ott', 'ott']
      ];
      vitalPadMap.forEach(([dsKey, paramId]) => {
        if (chip.dataset[dsKey] !== undefined) {
          const val = parseFloat(chip.dataset[dsKey]);
          if (!Number.isNaN(val)) {
            state.normalized[paramId] = val;
            updateKnobUI(paramId, val);
            sendFastParam(paramId, val);
          }
        }
      });
      if (chip.dataset.uni !== undefined) {
        const uv = parseInt(chip.dataset.uni, 10);
        if (!Number.isNaN(uv)) {
          updateUnisonVoicesUI(uv);
          sendIpc(`uni,${uv}`);
        }
      }

      const prompt = chip.dataset.prompt;
      input.value = prompt;
      state.isGenerating = true;
      updateGeneratingUI(true);
      sendFastGenerate(prompt);
      const auditionMidi = state.params.root_note || 60;
      pressNote(auditionMidi, padVel);
      setTimeout(() => releaseNote(auditionMidi), 260);
    });
  });

  initWavImportExport();
  initRootNoteControls();
  initLfoAndKeyzoneControls();
}

function updateGeneratingUI(isGenerating) {
  const btn = document.getElementById('btn-generate');
  if (!btn) return;
  const btnText = btn.querySelector('.btn-text');

  if (isGenerating) {
    btn.classList.add('busy');
    if (btnText) btnText.innerText = '[ SINTETIZANDO... ]';
  } else {
    btn.classList.remove('busy');
    if (btnText) btnText.innerText = '[ GENERAR IA ]';
  }
}

// -------------------------------------------------------------
// 10. AUDIO & MIDI HARDWARE SETTINGS MODAL (STANDALONE ONLY)
// -------------------------------------------------------------
function initAudioSettingsModal() {
  const btnOpen = document.getElementById('btn-audio-settings');
  const btnClose = document.getElementById('btn-close-settings');
  const btnCancel = document.getElementById('btn-cancel-settings');
  const btnSave = document.getElementById('btn-save-settings');
  const btnTestTone = document.getElementById('btn-test-tone');
  const modal = document.getElementById('settings-modal');
  const statusMsg = document.getElementById('settings-status-msg');
  const dawBanner = document.getElementById('daw-mode-banner');

  if (!btnOpen || !modal) return;

  function showStatus(text, isError = false) {
    if (!statusMsg) return;
    statusMsg.innerText = text;
    statusMsg.className = `settings-status-msg ${isError ? 'error' : ''}`;
  }

  function hideStatus() {
    if (!statusMsg) return;
    statusMsg.className = 'settings-status-msg hidden';
  }

  async function loadAudioDevices(forceFetch = false) {
    try {
      let data = (!forceFetch && window.__TEPUI_AUDIO_STATUS__) ? window.__TEPUI_AUDIO_STATUS__ : null;
      if (!data) {
        try {
          const resp = await fetch('/api/audio-devices');
          if (resp.ok) {
            data = await resp.json();
            window.__TEPUI_AUDIO_STATUS__ = data;
          }
        } catch (e) {
          console.warn('[AUDIO DEVICES FETCH ERROR]', e);
        }
      }
      if (!data) data = window.__TEPUI_AUDIO_STATUS__;
      if (!data) return;

      const remoteSkin = data.active_ui_skin || (data.config && data.config.ui_skin);
      let hasLocalSkin = false;
      try {
        hasLocalSkin = Boolean(localStorage.getItem('tepui_ui_skin'));
      } catch (_e) {}
      if (!hasLocalSkin && remoteSkin) {
        applyUiSkin(remoteSkin, false);
      }

      const isAudioTabHidden = paneAudio && paneAudio.classList.contains('hidden-pane');
      if (data.is_standalone || isAudioTabHidden) {
        if (dawBanner) dawBanner.classList.add('hidden');
        if (data.is_standalone && btnSave) btnSave.disabled = false;
      } else {
        if (dawBanner) dawBanner.classList.remove('hidden');
      }

      // Populate Output Devices
      const selectOut = document.getElementById('select-output-device');
      if (selectOut && data.output_devices) {
        selectOut.innerHTML = '';
        
        // Opción 1: Dispositivo por Defecto del Sistema (Windows Default)
        const defOption = document.createElement('option');
        defOption.value = '';
        const defDev = data.output_devices.find(d => d.is_default);
        defOption.innerText = `Por Defecto del Sistema (Windows Default)${defDev ? ` • ${defDev.name}` : ''}`;
        if (!data.active_output_device) {
          defOption.selected = true;
        }
        selectOut.appendChild(defOption);

        data.output_devices.forEach((dev) => {
          const opt = document.createElement('option');
          opt.value = dev.name;
          const isDef = dev.is_default ? ' [Por Defecto Windows]' : '';
          opt.innerText = `${dev.name}${isDef} (${dev.channels}ch)`;
          if (data.active_output_device && data.active_output_device === dev.name) {
            opt.selected = true;
          }
          selectOut.appendChild(opt);
        });
      }

      // Populate Input Devices (Sintetizador puro: siempre desactivado para evitar error WASAPI 0-ch)
      const selectIn = document.getElementById('select-input-device');
      if (selectIn) {
        selectIn.innerHTML = '<option value="" selected>Desactivado (Sintetizador — 0 canales de entrada)</option>';
        selectIn.value = '';
      }

      // Dynamic Sample Rate based on selected output device hardware capabilities
      function updateSampleRatesForDevice(devName) {
        const selectSr = document.getElementById('select-sample-rate');
        if (!selectSr || !data.output_devices) return;
        const dev = data.output_devices.find(d => d.name === devName) || data.output_devices.find(d => d.is_default);
        const supported = (dev && dev.supported_sample_rates && dev.supported_sample_rates.length > 0)
          ? dev.supported_sample_rates
          : [48000];

        selectSr.innerHTML = '';
        supported.forEach((rate) => {
          const opt = document.createElement('option');
          opt.value = String(rate);
          const khz = (rate / 1000).toFixed(1);
          opt.innerText = `${khz} kHz (${rate.toLocaleString()} Hz) [Nativo]`;
          selectSr.appendChild(opt);
        });

        const targetRate = data.active_sample_rate || 48000;
        if (supported.includes(targetRate)) {
          selectSr.value = String(targetRate);
        } else if (supported.length > 0) {
          selectSr.value = String(supported[0]);
        }
      }

      function getModalConfig() {
        return {
          output_device: selectOut && selectOut.value ? selectOut.value : null,
          input_device: null,
          sample_rate: selectSr ? parseInt(selectSr.value, 10) : 48000,
          period_size: selectBuf ? parseInt(selectBuf.value, 10) : 1056,
          midi_input: selectMidi && selectMidi.value ? selectMidi.value : null,
          ui_skin: currentUiSkin || 'minimal',
        };
      }

      function updateSettingsLatencyBadge() {
        const srVal = selectSr && selectSr.value ? parseInt(selectSr.value, 10) : 48000;
        const bufVal = selectBuf && selectBuf.value ? parseInt(selectBuf.value, 10) : 1056;
        const ms = ((bufVal / Math.max(8000, srVal)) * 1000).toFixed(1);
        const badge = document.getElementById('settings-latency-badge');
        if (badge) {
          badge.innerText = `${(srVal / 1000).toFixed(1)} kHz • ${bufVal} smp (~${ms} ms)`;
        }
      }

      function autoSaveConfig() {
        updateSettingsLatencyBadge();
        const cfg = getModalConfig();
        if (window.__TEPUI_AUDIO_STATUS__) {
          window.__TEPUI_AUDIO_STATUS__.active_output_device = cfg.output_device;
          window.__TEPUI_AUDIO_STATUS__.active_input_device = cfg.input_device;
          window.__TEPUI_AUDIO_STATUS__.active_sample_rate = cfg.sample_rate;
          window.__TEPUI_AUDIO_STATUS__.active_period_size = cfg.period_size;
          window.__TEPUI_AUDIO_STATUS__.active_midi_input = cfg.midi_input;
          window.__TEPUI_AUDIO_STATUS__.active_ui_skin = cfg.ui_skin;
        }
        sendIpc('save_audio_config:' + JSON.stringify(cfg));
      }

      if (selectOut) {
        selectOut.onchange = () => {
          updateSampleRatesForDevice(selectOut.value);
          autoSaveConfig();
          showStatus('[!] Configuración guardada. Para activar el nuevo dispositivo de audio pulsa "GUARDAR Y REINICIAR MOTOR" abajo.', false);
        };
        const currentDev = selectOut.value || (data.active_output_device || (data.output_devices[0] && data.output_devices[0].name));
        if (currentDev) updateSampleRatesForDevice(currentDev);
      }

      if (selectIn) {
        selectIn.onchange = autoSaveConfig;
      }

      // Buffer Size
      const selectBuf = document.getElementById('select-buffer-size');
      if (selectBuf) {
        if (data.active_period_size) {
          selectBuf.value = String(data.active_period_size);
        }
        selectBuf.onchange = autoSaveConfig;
      }

      const selectSr = document.getElementById('select-sample-rate');
      if (selectSr) {
        selectSr.onchange = autoSaveConfig;
      }
      updateSettingsLatencyBadge();

      // MIDI Inputs
      const selectMidi = document.getElementById('select-midi-device');
      if (selectMidi) {
        selectMidi.innerHTML = '<option value="">Ninguno / Teclado Virtual</option>';
        if (data.midi_inputs && data.midi_inputs.length > 0) {
          data.midi_inputs.forEach((port) => {
            const opt = document.createElement('option');
            opt.value = port;
            opt.innerText = port;
            if (data.active_midi_input && data.active_midi_input === port) {
              opt.selected = true;
            }
            selectMidi.appendChild(opt);
          });
        } else {
          const opt = document.createElement('option');
          opt.value = "";
          opt.innerText = "No se detectaron teclados MIDI USB";
          selectMidi.appendChild(opt);
        }
        selectMidi.onchange = () => {
          autoSaveConfig();
          if (selectMidi.value) {
            showStatus('[OK] Controlador MIDI USB "' + selectMidi.value + '" conectado en caliente (Hot-Plug activo, sin reinicio).', false);
          } else {
            showStatus('[OK] Entrada MIDI USB desconectada (usando teclado virtual / host DAW).', false);
          }
        };
      }
    } catch (e) {
      console.warn('[TEPUI AUDIO SETTINGS FETCH ERROR]', e);
      showStatus('No se pudieron leer los dispositivos: ' + e.message, true);
    }
  }

  const tabBtnAudio = document.getElementById('settings-tab-audio');
  const tabBtnAi = document.getElementById('settings-tab-ai');
  const tabBtnSkin = document.getElementById('settings-tab-skin');
  const tabBtnAll = document.getElementById('settings-tab-all');
  const paneAudio = document.getElementById('settings-pane-audio');
  const paneAi = document.getElementById('settings-pane-ai');
  const paneSkin = document.getElementById('settings-pane-skin');

  const switchSettingsTab = (tabName) => {
    [tabBtnAudio, tabBtnAi, tabBtnSkin, tabBtnAll].forEach((b) => {
      if (!b) return;
      const isMatch = b.dataset.tab === tabName;
      b.classList.toggle('active', isMatch);
      b.setAttribute('aria-selected', isMatch ? 'true' : 'false');
    });
    if (paneAudio) {
      paneAudio.classList.toggle('hidden-pane', tabName !== 'audio' && tabName !== 'all');
    }
    if (paneAi) {
      paneAi.classList.toggle('hidden-pane', tabName !== 'ai' && tabName !== 'all');
    }
    if (paneSkin) {
      paneSkin.classList.toggle('hidden-pane', tabName !== 'skin' && tabName !== 'all');
    }
    if (dawBanner) {
      const isStandalone = window.__TEPUI_AUDIO_STATUS__ ? Boolean(window.__TEPUI_AUDIO_STATUS__.is_standalone) : true;
      dawBanner.classList.toggle('hidden', isStandalone || (tabName !== 'audio' && tabName !== 'all'));
    }
  };

  if (tabBtnAudio) tabBtnAudio.addEventListener('click', () => switchSettingsTab('audio'));
  if (tabBtnAi) tabBtnAi.addEventListener('click', () => switchSettingsTab('ai'));
  if (tabBtnSkin) tabBtnSkin.addEventListener('click', () => switchSettingsTab('skin'));
  if (tabBtnAll) tabBtnAll.addEventListener('click', () => switchSettingsTab('all'));

  const openSettingsModal = (initialTab = 'audio') => {
    hideStatus();
    switchSettingsTab(initialTab);
    modal.classList.remove('hidden');
    loadAudioDevices(true);
    loadCloudLicenseState();
  };

  window.__tepuiOpenSettingsTab = openSettingsModal;
  window.__tepuiPersistSkinConfig = (skinId) => {
    const selectOut = document.getElementById('select-output-device');
    const selectIn = document.getElementById('select-input-device');
    const selectSr = document.getElementById('select-sample-rate');
    const selectBuf = document.getElementById('select-buffer-size');
    const selectMidi = document.getElementById('select-midi-device');
    const cfg = {
      output_device: selectOut && selectOut.value ? selectOut.value : null,
      input_device: selectIn && selectIn.value ? selectIn.value : null,
      sample_rate: selectSr && selectSr.value ? parseInt(selectSr.value, 10) : 48000,
      period_size: selectBuf && selectBuf.value ? parseInt(selectBuf.value, 10) : 1056,
      midi_input: selectMidi && selectMidi.value ? selectMidi.value : null,
      ui_skin: skinId || currentUiSkin || 'minimal',
    };
    if (window.__TEPUI_AUDIO_STATUS__) {
      window.__TEPUI_AUDIO_STATUS__.active_ui_skin = cfg.ui_skin;
    }
    sendIpc('save_audio_config:' + JSON.stringify(cfg));
    const found = UI_SKINS.find((s) => s.id === cfg.ui_skin);
    if (found && !modal.classList.contains('hidden')) {
      showStatus(`[OK] Skin "${found.fullLabel}" (${found.className}) aplicado instantáneamente y persistido.`);
    }
  };

  btnOpen.addEventListener('click', () => openSettingsModal('audio'));
  const btnCloudCredits = document.getElementById('btn-cloud-credits');
  if (btnCloudCredits) {
    btnCloudCredits.addEventListener('click', () => openSettingsModal('ai'));
  }

  async function loadCloudLicenseState() {
    try {
      const resp = await fetch('/api/cloud-license');
      if (!resp.ok) return;
      const data = await resp.json();
      const cfg = data.config || {};
      const quota = data.quota || {};
      if (cfg.device_id) {
        state.deviceId = cfg.device_id;
      }
      const provSel = document.getElementById('select-ai-provider');
      const modelInp = document.getElementById('input-ai-model');
      const keyInp = document.getElementById('input-ai-api-key');
      const endpInp = document.getElementById('input-ai-endpoint');
      if (provSel) {
        provSel.value = cfg.local_npu_only ? 'local_npu' : (cfg.ai_provider || 'community_edge');
      }
      if (modelInp) modelInp.value = cfg.model_name || '';
      if (keyInp) keyInp.value = cfg.api_key || '';
      if (endpInp) {
        endpInp.value =
          cfg.ai_gateway_url ||
          cfg.custom_endpoint ||
          'https://tepui-vst.pc911itservices.workers.dev/v1/sample-wav';
      }
      if (state.oddMetrics && quota.tier) {
        state.oddMetrics.cloud_tier = quota.tier;
        state.oddMetrics.cloud_credits_remaining = quota.credits_remaining;
        state.oddMetrics.cloud_daily_neurons = quota.daily_neurons_used || quota.daily_neurons || 0;
      }
      refreshFooterOddText();
    } catch (e) {
      console.warn('[TEPUI AI PROVIDER FETCH]', e);
    }
  }

  const btnVerifyCloud = document.getElementById('btn-verify-cloud-license');
  if (btnVerifyCloud) {
    btnVerifyCloud.addEventListener('click', async () => {
      const provSel = document.getElementById('select-ai-provider');
      const modelInp = document.getElementById('input-ai-model');
      const keyInp = document.getElementById('input-ai-api-key');
      const endpInp = document.getElementById('input-ai-endpoint');
      const selectedProv = provSel && provSel.value ? provSel.value : 'community_edge';
      const gwUrl =
        endpInp && endpInp.value.trim()
          ? endpInp.value.trim()
          : 'https://tepui-vst.pc911itservices.workers.dev/v1/sample-wav';
      const payload = {
        ai_provider: selectedProv,
        api_key: keyInp && keyInp.value.trim() ? keyInp.value.trim() : '',
        custom_endpoint: gwUrl,
        ai_gateway_url: gwUrl,
        wav_generation_enabled: selectedProv !== 'local_npu',
        model_name: modelInp && modelInp.value.trim() ? modelInp.value.trim() : '',
        license_key: '',
        device_id: '',
        byok_account_id: '',
        byok_api_token: '',
        local_npu_only: selectedProv === 'local_npu'
      };
      showStatus('Guardando proveedor de IA y TEPUI_AI_GATEWAY_URL...');
      try {
        const resp = await fetch('/api/save-cloud-license', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(payload)
        });
        if (resp.ok) {
          const res = await resp.json();
          if (res.quota) {
            if (!state.oddMetrics) state.oddMetrics = {};
            state.oddMetrics.cloud_tier = res.quota.tier;
            state.oddMetrics.cloud_credits_remaining = res.quota.credits_remaining;
            state.oddMetrics.cloud_daily_neurons = res.quota.daily_neurons_used || res.quota.daily_neurons || 0;
            refreshFooterOddText();
            showStatus(`[OK] Proveedor IA activo: ${res.quota.tier} (${res.quota.message || 'Listo para sintetizar'})`);
            return;
          }
        }
      } catch (e) {
        console.warn('[TEPUI SAVE AI PROVIDER]', e);
        sendIpc('save_cloud_license:' + JSON.stringify(payload));
      }
      showStatus('[OK] Proveedor de IA guardado exitosamente.');
    });
  }

  const btnBuyCloud = document.getElementById('btn-buy-cloud-credits');
  if (btnBuyCloud) {
    btnBuyCloud.addEventListener('click', () => {
      const devId = encodeURIComponent(state.deviceId || 'global');
      const checkoutUrl = `https://tepui-vst.pc911itservices.workers.dev/?client_reference_id=${devId}#pricing`;
      window.open(checkoutUrl, '_blank', 'noopener,noreferrer');
      showStatus(
        `[OK] Abriendo Checkout seguro vinculado a tu equipo (${state.deviceId || 'global'}). Al completarse el pago, el webhook HMAC-SHA256 acreditará tus tokens automáticamente.`
      );
    });
  }

  const btnRedeemCloud = document.getElementById('btn-redeem-cloud-code');
  if (btnRedeemCloud) {
    btnRedeemCloud.addEventListener('click', async () => {
      const codeInp = document.getElementById('input-cloud-redeem-code');
      const code = codeInp && codeInp.value ? codeInp.value.trim() : '';
      if (!code) {
        showStatus('Ingresa un código de recarga o licencia Cloud (ej. TEPUI-PRO-2026).', true);
        return;
      }
      showStatus('Verificando y acreditando código en TEPUI_CREDITS_KV...');
      try {
        const resp = await fetch('/api/redeem-cloud-code', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ code })
        });
        const res = await resp.json();
        if (resp.ok && (res.ok || res.success)) {
          if (!state.oddMetrics) state.oddMetrics = {};
          state.oddMetrics.cloud_credits_remaining = res.credits_remaining;
          if (res.tier) state.oddMetrics.cloud_tier = res.tier;
          refreshFooterOddText();
          if (codeInp) codeInp.value = '';
          showStatus(`[OK] ${res.message || 'Código activado.'} Saldo actual: ${res.credits_remaining} CR.`);
          return;
        }
        showStatus(res.error || 'Código inválido o ya utilizado.', true);
        return;
      } catch (e) {
        console.warn('[TEPUI REDEEM CLOUD CODE]', e);
        sendIpc('redeem_cloud_code:' + code);
      }
      showStatus('[OK] Solicitud de canje enviada al motor Cloud.');
    });
  }

  const closeModal = () => {
    modal.classList.add('hidden');
    hideStatus();
  };

  if (btnClose) btnClose.addEventListener('click', closeModal);
  if (btnCancel) btnCancel.addEventListener('click', closeModal);

  // Close on clicking backdrop outside card
  modal.addEventListener('click', (e) => {
    if (e.target === modal) closeModal();
  });

  // Test Tone: 100% Rust DSP Puro
  if (btnTestTone) {
    btnTestTone.addEventListener('click', () => {
      const textSpan = document.getElementById('test-tone-text');
      btnTestTone.classList.add('active');
      if (textSpan) textSpan.innerText = 'SONANDO (C4-E4-G4)...';

      // Disparar acorde interactivo directamente en motor de audio nativo Rust
      sendFastNoteOn(60, 1.0);
      sendFastNoteOn(64, 0.9);
      sendFastNoteOn(67, 0.95);

      setTimeout(() => {
        sendFastNoteOff(60);
        sendFastNoteOff(64);
        sendFastNoteOff(67);
        btnTestTone.classList.remove('active');
        if (textSpan) textSpan.innerText = 'PROBAR SONIDO (TEST CHORD)';
      }, 900);
    });
  }

  // Save Settings & Restart
  if (btnSave) {
    btnSave.addEventListener('click', async () => {
      const selectOut = document.getElementById('select-output-device');
      const selectIn = document.getElementById('select-input-device');
      const selectSr = document.getElementById('select-sample-rate');
      const selectBuf = document.getElementById('select-buffer-size');
      const selectMidi = document.getElementById('select-midi-device');

      const config = {
        output_device: selectOut && selectOut.value ? selectOut.value : null,
        input_device: selectIn && selectIn.value ? selectIn.value : null,
        sample_rate: selectSr ? parseInt(selectSr.value, 10) : 48000,
        period_size: selectBuf ? parseInt(selectBuf.value, 10) : 1056,
        midi_input: selectMidi && selectMidi.value ? selectMidi.value : null,
        ui_skin: currentUiSkin || 'minimal',
      };

      showStatus('Guardando configuración de audio...');

      if (window.__TEPUI_AUDIO_STATUS__) {
        window.__TEPUI_AUDIO_STATUS__.active_output_device = config.output_device;
        window.__TEPUI_AUDIO_STATUS__.active_input_device = config.input_device;
        window.__TEPUI_AUDIO_STATUS__.active_sample_rate = config.sample_rate;
        window.__TEPUI_AUDIO_STATUS__.active_period_size = config.period_size;
        window.__TEPUI_AUDIO_STATUS__.active_midi_input = config.midi_input;
        window.__TEPUI_AUDIO_STATUS__.active_ui_skin = config.ui_skin;
      }

      sendIpc('save_audio_config:' + JSON.stringify(config));
      showStatus('[OK] Configuración guardada. Reiniciando motor de audio...');
      setTimeout(() => {
        sendIpc('restart_standalone');
      }, 400);
    });
  }
}

// -------------------------------------------------------------
// 11. CATEGORIZED SOUND PACK & 1,250+ PRESET BROWSER ENGINE
// -------------------------------------------------------------
const SOUND_PACKS = [
  { id: 'ALL', code: 'ALL PACKS', title: 'All Sound Banks', icon: '<svg class="hw-svg" viewBox="0 0 16 16"><path d="M1.5 13L5.5 5l3 4 2.5-3.5L14.5 13h-13z"/></svg>', bg: 'linear-gradient(135deg,#00ebff,#0f172a)' },
  { id: 'pack-01', code: 'Pack 01:', title: 'Vital Wavetables & FM', icon: '<svg class="hw-svg" viewBox="0 0 16 16"><path d="M3 2v12M7 3v10M11 4v8M14 6v4"/></svg>', bg: 'linear-gradient(135deg,#f59e0b,#78350f)' },
  { id: 'pack-02', code: 'Pack 02:', title: 'Reese, Acid & 808 Bass', icon: '<svg class="hw-svg" viewBox="0 0 16 16"><ellipse cx="8" cy="5" rx="5" ry="2.2"/><path d="M3 5v6c0 1.2 2.2 2.2 5 2.2s5-1 5-2.2V5"/></svg>', bg: 'linear-gradient(135deg,#06b6d4,#164e63)' },
  { id: 'pack-03', code: 'Pack 03:', title: 'Cinematic & Vocal Pads', icon: '<svg class="hw-svg" viewBox="0 0 16 16"><path d="M1 12l3-6h8l3 6H1zM4 6l1.5-2.5h5L12 6"/></svg>', bg: 'linear-gradient(135deg,#10b981,#064e3b)' },
  { id: 'pack-04', code: 'Pack 04:', title: 'Supersaw & Cyber Leads', icon: '<svg class="hw-svg" viewBox="0 0 16 16"><path d="M1 10l3.5-5v5l3.5-5v5l3.5-5v5L15 5"/></svg>', bg: 'linear-gradient(135deg,#00ebff,#1e3a8a)' },
  { id: 'pack-05', code: 'Pack 05:', title: 'Karplus Acoustic Strings', icon: '<svg class="hw-svg" viewBox="0 0 16 16"><path d="M1 8c2-5 3.5-5 5.5 0s3.5 5 5.5 0 2-3 3-3"/></svg>', bg: 'linear-gradient(135deg,#e05a47,#451a03)' }
];

const MASTER_PRESET_SEEDS = [
  { name: 'Neuro Reese Sub', pack: 'pack-02', cat: 'Basses', tags: 'Reese, Neuro, Basses', rhythm: '4/4', arp: 0, prompt: 'Neuro Reese Bass Sub C2', root: 36, norm: { attack: 0.015, decay: 0.35, sustain: 0.88, release: 0.32, cutoff: 0.54, resonance: 0.28, space_mix: 0.14, lfo_rate: 0.28, lfo_depth: 0.32, wt_pos: 0.72, wt_warp: 0.38, unison_detune: 0.45, sub_level: 0.65, drive: 0.42, ott: 0.55 } },
  { name: 'Hyper Supersaw Lead', pack: 'pack-04', cat: 'Leads', tags: 'Supersaw, Unison, Leads', rhythm: '4/4', arp: 0, prompt: 'Hyper Supersaw Anthem Lead C4', root: 60, norm: { attack: 0.005, decay: 0.28, sustain: 0.85, release: 0.34, cutoff: 0.92, resonance: 0.16, space_mix: 0.38, lfo_rate: 0.35, lfo_depth: 0.10, wt_pos: 0.48, wt_warp: 0.18, unison_detune: 0.58, sub_level: 0.25, drive: 0.22, ott: 0.48 } },
  { name: 'Acid 303 Squelch', pack: 'pack-02', cat: 'Basses', tags: 'Acid 303, TB-303, Basses', rhythm: '4/4', arp: 1, prompt: 'Resonant Acid 303 Squelch C2', root: 36, norm: { attack: 0.002, decay: 0.22, sustain: 0.42, release: 0.18, cutoff: 0.62, resonance: 0.68, space_mix: 0.16, lfo_rate: 0.45, lfo_depth: 0.22, wt_pos: 0.64, wt_warp: 0.52, unison_detune: 0.10, sub_level: 0.20, drive: 0.58, ott: 0.35 } },
  { name: 'Crystal Glass FM Pluck', pack: 'pack-01', cat: 'Plucks', tags: 'FM Pluck, Crystal, Wavetable', rhythm: '4/4', arp: 0, prompt: 'Crystal Glass FM Pluck C4', root: 60, norm: { attack: 0.002, decay: 0.24, sustain: 0.55, release: 0.32, cutoff: 0.94, resonance: 0.15, space_mix: 0.36, lfo_rate: 0.32, lfo_depth: 0.08, wt_pos: 0.88, wt_warp: 0.44, unison_detune: 0.22, sub_level: 0.10, drive: 0.12, ott: 0.42 } },
  { name: 'Ethereal Formant Choir', pack: 'pack-03', cat: 'Pads', tags: 'Formant, Vocal Pad, Choir', rhythm: '4/4', arp: 0, prompt: 'Ethereal Formant Vocal Pad C4', root: 60, norm: { attack: 0.38, decay: 0.52, sustain: 0.92, release: 0.64, cutoff: 0.84, resonance: 0.18, space_mix: 0.62, lfo_rate: 0.22, lfo_depth: 0.25, wt_pos: 0.35, wt_warp: 0.28, unison_detune: 0.42, sub_level: 0.18, drive: 0.08, ott: 0.32 } },
  { name: 'Salto Ángel Shimmer Pad', pack: 'pack-03', cat: 'Pads', tags: 'Pads, Ambient, Shimmer', rhythm: '4/4', arp: 0, prompt: 'Bright Crystal Shimmer Choir Pad E4', root: 64, norm: { attack: 0.42, decay: 0.55, sustain: 0.90, release: 0.65, cutoff: 0.82, resonance: 0.16, space_mix: 0.68, lfo_rate: 0.22, lfo_depth: 0.28 } },
  { name: 'Catatumbo 808 Sub', pack: 'pack-02', cat: 'Basses', tags: 'Basses, 808 Sub', rhythm: '4/4', arp: 0, prompt: 'Deep 808 Sub Bass', root: 36, norm: { attack: 0.002, decay: 0.28, sustain: 0.75, release: 0.32, cutoff: 0.38, resonance: 0.25, space_mix: 0.08, lfo_rate: 0.20, lfo_depth: 0.0, sub_level: 0.85, drive: 0.38, ott: 0.30 } },
  { name: 'Cyberpunk Neon Arp', pack: 'pack-04', cat: 'Leads', tags: 'Synthwave, Plucks, Arp', rhythm: '4/4', arp: 1, prompt: 'Cyberpunk Pluck Lead Synth', root: 60, norm: { attack: 0.002, decay: 0.20, sustain: 0.68, release: 0.30, cutoff: 0.85, resonance: 0.28, space_mix: 0.35, lfo_rate: 0.48, lfo_depth: 0.18, wt_pos: 0.56, wt_warp: 0.34, unison_detune: 0.36, drive: 0.28, ott: 0.45 } },
  { name: 'Karplus Nylon Guitar', pack: 'pack-05', cat: 'Plucks', tags: 'Karplus-Strong, Nylon, Acoustic', rhythm: '6/8', arp: 3, prompt: 'Karplus Nylon Guitar Pluck C4', root: 60, norm: { attack: 0.002, decay: 0.22, sustain: 0.62, release: 0.26, cutoff: 0.90, resonance: 0.12, space_mix: 0.22, lfo_rate: 0.35, lfo_depth: 0.05 } },
  { name: 'Karplus Concert Harp', pack: 'pack-05', cat: 'Strings', tags: 'Karplus-Strong, Harp, Strings', rhythm: '6/8', arp: 3, prompt: 'Karplus Concert Harp Pluck C4', root: 60, norm: { attack: 0.002, decay: 0.28, sustain: 0.68, release: 0.35, cutoff: 0.92, resonance: 0.14, space_mix: 0.28, lfo_rate: 0.35, lfo_depth: 0.08 } },
  { name: 'Karplus Plectrum Lute', pack: 'pack-05', cat: 'Plucks', tags: 'Karplus-Strong, Bandola, Plucks', rhythm: '6/8', arp: 3, prompt: 'Karplus Plectrum Lute Pluck A3', root: 57, norm: { attack: 0.002, decay: 0.24, sustain: 0.70, release: 0.28, cutoff: 0.88, resonance: 0.18, space_mix: 0.22, lfo_rate: 0.42, lfo_depth: 0.10 } },
  { name: 'Kukenan Crystal Bell', pack: 'pack-01', cat: 'Plucks', tags: 'Plucks, FM Bell, Wavetable', rhythm: '5/8', arp: 4, prompt: 'Metallic FM Bell Chime', root: 72, norm: { attack: 0.001, decay: 0.32, sustain: 0.50, release: 0.48, cutoff: 0.94, resonance: 0.16, space_mix: 0.45, lfo_rate: 0.30, lfo_depth: 0.10 } },
  { name: 'Auyantepui Warm Analog', pack: 'pack-03', cat: 'Pads', tags: 'Pads, Warm Analog', rhythm: '4/4', arp: 0, prompt: 'Warm Analog Synth Pad', root: 60, norm: { attack: 0.32, decay: 0.45, sustain: 0.88, release: 0.58, cutoff: 0.72, resonance: 0.15, space_mix: 0.52, lfo_rate: 0.25, lfo_depth: 0.22 } },
  { name: 'Canaima Brass Ensemble', pack: 'pack-04', cat: 'Leads', tags: 'Leads, Brass, Analog', rhythm: '4/4', arp: 1, prompt: 'Vintage Brass Ensemble Horns', root: 60, norm: { attack: 0.06, decay: 0.28, sustain: 0.82, release: 0.35, cutoff: 0.80, resonance: 0.18, space_mix: 0.32, lfo_rate: 0.34, lfo_depth: 0.12 } }
];

let PRESET_CATALOG = [];
let activePackFilter = 'ALL';
let activeCatFilter = 'ALL';
let activePresetId = 'p-0';
const favoritePresets = new Set(['p-0', 'p-3', 'p-5']);

function buildPresetCatalog() {
  if (PRESET_CATALOG.length > 0) return;
  const adjectives = [
    'Roraima', 'Canaima', 'Apure', 'Barlovento', 'Orinoco', 'Kukenan', 'Auyantepui',
    'Catatumbo', 'Caroni', 'Caracas', 'Zulia', 'Guayana', 'Autana', 'Churun', 'Morichal',
    'Sabana', 'Cinaruco', 'Arauca', 'Yuruani', 'Kamarata', 'Jaspe', 'Macagua', 'Paragua', 'Turimiquire', 'Avila'
  ];
  const variations = [
    'Cristal', 'Recio', 'Nocturno', 'Solar', 'Profundo', 'Ancestral', 'Espectral',
    'Dorado', 'Neblina', 'Salvaje', ' Eléctrico', 'Acústico', 'Cálido', 'Brillante', 'Sub',
    'Polirrítmico', 'Sinfónico', 'Cinta Vintage', 'Resonante', 'Modular'
  ];

  // 1. Insert flagship seeds first
  MASTER_PRESET_SEEDS.forEach((seed, idx) => {
    PRESET_CATALOG.push({
      id: `p-${idx}`,
      ...seed
    });
  });

  // 2. Expand deterministically to 1,250 categorized presets across all 5 Sound Packs
  let idCounter = PRESET_CATALOG.length;
  for (let i = 0; idCounter < 1250; i++) {
    const base = MASTER_PRESET_SEEDS[i % MASTER_PRESET_SEEDS.length];
    const adj = adjectives[(i * 7 + 3) % adjectives.length];
    const vari = variations[(i * 11 + 5) % variations.length];
    const packIdx = (i % 5) + 1;
    const packId = `pack-0${packIdx}`;
    const detuneNote = base.root + ((i % 5) - 2);
    const noteStr = midiNoteToName(detuneNote);
    const cutShift = (((i * 17) % 30) - 15) * 0.01;
    const spcShift = (((i * 13) % 24) - 12) * 0.01;

    PRESET_CATALOG.push({
      id: `p-${idCounter}`,
      name: `${adj} ${base.name.split(' ')[0]} ${vari} #${idCounter + 1}`,
      pack: packId,
      cat: base.cat,
      tags: `${base.tags}, ${adj}`,
      rhythm: base.rhythm,
      arp: base.arp,
      prompt: `${base.prompt} ${adj} ${vari} ${noteStr}`,
      root: detuneNote,
      norm: {
        attack: Math.max(0.001, Math.min(0.85, base.norm.attack)),
        decay: Math.max(0.05, Math.min(0.85, base.norm.decay)),
        sustain: Math.max(0.20, Math.min(0.95, base.norm.sustain)),
        release: Math.max(0.08, Math.min(0.88, base.norm.release)),
        cutoff: Math.max(0.25, Math.min(0.99, base.norm.cutoff + cutShift)),
        resonance: Math.max(0.06, Math.min(0.65, base.norm.resonance)),
        space_mix: Math.max(0.05, Math.min(0.85, base.norm.space_mix + spcShift)),
        lfo_rate: base.norm.lfo_rate,
        lfo_depth: base.norm.lfo_depth
      }
    });
    idCounter++;
  }
}

function applyPresetFromCatalog(preset, triggerAudition = true) {
  if (!preset) return;
  saveSnapshotBeforeChange();
  activePresetId = preset.id;
  synthStore.setActiveChipId('');
  synthStore.setActivePadId('');
  document.querySelectorAll('.chip').forEach(c => c.classList.remove('active'));

  const promptInput = document.getElementById('prompt-input');
  if (promptInput) promptInput.value = preset.prompt;

  const browserLbl = document.getElementById('preset-browser-label');
  if (browserLbl) {
    browserLbl.innerText = `PRESET: ${preset.name.toUpperCase()} (1,250)`;
  }

  // Aplicar perfil DSP macro y matriz 5x5 instantáneamente via IPC
  const presetMask = preset.matrixMask !== undefined
    ? preset.matrixMask
    : ((1 << ((parseInt(preset.id.replace(/\D+/g, ''), 10) || 0) % 5)) | (1 << 6) | (1 << 11));
  syncMatrixPinsFromMask(presetMask);

  if (preset.norm) {
    for (const [k, v] of Object.entries(preset.norm)) {
      state.normalized[k] = v;
      updateKnobUI(k, v);
      sendFastParam(k, v);
    }
  }
  if (preset.arp !== undefined) {
    updateArpModeUI(preset.arp);
    sendIpc(`arp,${preset.arp}`);
  }
  if (preset.root !== undefined) {
    sendFastRootNote(preset.root);
  }

  // Registrar evento ODD de aplicación paramétrica completa y sintetizar en el motor IA/Físico con caché NPU
  state.localUiPresetLoads = (state.localUiPresetLoads || 0) + 1;
  const applyPayload = {
    prompt: preset.prompt,
    arp_mode: preset.arp !== undefined ? preset.arp : 0,
    root_note: preset.root !== undefined ? preset.root : 60,
    mod_matrix_mask: presetMask
  };
  sendIpc(`preset_apply:${JSON.stringify(applyPayload)}`);
  refreshFooterOddText();
  pollAiGeneration();
  renderPresetTable();

  if (triggerAudition && state.presetAutoAudition !== false) {
    const auditionMidi = preset.root !== undefined ? preset.root : (state.params.root_note || 60);
    pressNote(auditionMidi, 0.88);
    setTimeout(() => releaseNote(auditionMidi), 240);
  }
}

function renderSoundPacks() {
  const container = document.getElementById('sound-pack-list');
  if (!container) return;
  container.innerHTML = '';

  SOUND_PACKS.forEach((p) => {
    const count = p.id === 'ALL'
      ? PRESET_CATALOG.length
      : PRESET_CATALOG.filter(item => item.pack === p.id).length;

    const card = document.createElement('div');
    card.className = `sound-pack-card ${activePackFilter === p.id ? 'active' : ''}`;
    card.innerHTML = `
      <div class="pack-art" style="background:${p.bg}">${p.icon}</div>
      <div class="pack-info">
        <span class="pack-code">${p.code}</span>
        <span class="pack-title">${p.title}</span>
        <span class="pack-count">${count.toLocaleString()} Presets</span>
      </div>
    `;
    card.addEventListener('click', () => {
      activePackFilter = p.id;
      document.querySelectorAll('.pack-pill').forEach(pill => {
        pill.classList.toggle('active', pill.dataset.pack === p.id);
      });
      renderSoundPacks();
      renderPresetTable();
    });
    container.appendChild(card);
  });
}

function renderPresetTable() {
  const tbody = document.getElementById('preset-table-body');
  const searchInput = document.getElementById('preset-search-input');
  const countBadge = document.getElementById('preset-count-badge');
  if (!tbody) return;

  const q = (searchInput && searchInput.value ? searchInput.value : '').trim().toLowerCase();

  const filtered = PRESET_CATALOG.filter((p) => {
    if (activePackFilter !== 'ALL' && p.pack !== activePackFilter) return false;
    if (activeCatFilter === 'FAV' && !favoritePresets.has(p.id)) return false;
    if (activeCatFilter !== 'ALL' && activeCatFilter !== 'FAV' && p.cat !== activeCatFilter) return false;
    if (q) {
      const hay = `${p.name} ${p.tags} ${p.cat} ${p.rhythm} ${p.prompt}`.toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });

  if (countBadge) countBadge.innerText = filtered.length.toLocaleString();

  // Renderizar las primeras 120 coincidencias en el DOM para scroll instantáneo a 60 FPS
  const slice = filtered.slice(0, 120);
  tbody.innerHTML = '';

  slice.forEach((p) => {
    const isFav = favoritePresets.has(p.id);
    const meterClass = p.rhythm === '6/8' ? 'm-68' : (p.rhythm === '5/8' ? 'm-58' : '');
    const row = document.createElement('div');
    row.className = `preset-row-item ${activePresetId === p.id ? 'active' : ''}`;
    row.innerHTML = `
      <span class="p-fav-btn ${isFav ? 'fav' : ''}" title="Marcar como favorito">◆</span>
      <span class="p-name">${p.name}</span>
      <span class="p-genre">${p.tags}</span>
      <span class="p-meter-badge ${meterClass}">${p.rhythm}</span>
    `;

    const favBtn = row.querySelector('.p-fav-btn');
    if (favBtn) {
      favBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        if (favoritePresets.has(p.id)) favoritePresets.delete(p.id);
        else favoritePresets.add(p.id);
        state.localUiFavToggles = (state.localUiFavToggles || 0) + 1;
        sendIpc(`fav_toggle:${p.id}`);
        refreshFooterOddText();
        renderPresetTable();
      });
    }

    row.addEventListener('click', () => {
      applyPresetFromCatalog(p);
    });

    tbody.appendChild(row);
  });
}

function initPresetBrowser() {
  buildPresetCatalog();

  const drawer = document.getElementById('preset-browser-drawer');
  const btnToggle = document.getElementById('btn-preset-browser');
  const btnPrev = document.getElementById('btn-preset-prev');
  const btnNext = document.getElementById('btn-preset-next');
  const btnClose = document.getElementById('btn-close-browser');
  const searchInput = document.getElementById('preset-search-input');

  const btnAudition = document.getElementById('btn-preset-audition');
  state.presetAutoAudition = true;
  if (btnAudition) {
    btnAudition.addEventListener('click', () => {
      state.presetAutoAudition = !state.presetAutoAudition;
      btnAudition.classList.toggle('active', state.presetAutoAudition);
      btnAudition.innerText = state.presetAutoAudition ? '🔊 AUTO' : '🔇 MUTE';
    });
  }

  const stepPreset = (delta) => {
    const q = (searchInput && searchInput.value ? searchInput.value : '').trim().toLowerCase();
    const list = PRESET_CATALOG.filter((p) => {
      if (activePackFilter !== 'ALL' && p.pack !== activePackFilter) return false;
      if (activeCatFilter === 'FAV' && !favoritePresets.has(p.id)) return false;
      if (activeCatFilter !== 'ALL' && activeCatFilter !== 'FAV' && p.cat !== activeCatFilter) return false;
      if (q) {
        const hay = `${p.name} ${p.tags} ${p.cat} ${p.rhythm} ${p.prompt}`.toLowerCase();
        if (!hay.includes(q)) return false;
      }
      return true;
    });
    if (list.length === 0) return;
    const curIdx = Math.max(0, list.findIndex((p) => p.id === activePresetId));
    const nextIdx = (curIdx + delta + list.length) % list.length;
    applyPresetFromCatalog(list[nextIdx], true);
    requestAnimationFrame(() => {
      const activeRow = document.querySelector('#preset-table-body .preset-row-item.active');
      if (activeRow && typeof activeRow.scrollIntoView === 'function') {
        activeRow.scrollIntoView({ block: 'nearest' });
      }
    });
  };

  window.addEventListener('keydown', (e) => {
    if (!drawer || drawer.classList.contains('hidden')) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      stepPreset(1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      stepPreset(-1);
    }
  });

  if (btnPrev) btnPrev.addEventListener('click', () => stepPreset(-1));
  if (btnNext) btnNext.addEventListener('click', () => stepPreset(1));

  const setBrowserOpen = (open) => {
    if (!drawer) return;
    drawer.classList.toggle('hidden', !open);
    if (btnToggle) btnToggle.classList.toggle('active', open);
    if (open) {
      renderSoundPacks();
      renderPresetTable();
    }
  };

  if (btnToggle && drawer) {
    btnToggle.addEventListener('click', () => {
      const isHidden = drawer.classList.contains('hidden');
      setBrowserOpen(isHidden);
    });
  }

  if (btnClose) {
    btnClose.addEventListener('click', () => setBrowserOpen(false));
  }

  if (searchInput) {
    searchInput.addEventListener('input', () => renderPresetTable());
  }

  document.querySelectorAll('.cat-pill').forEach((pill) => {
    pill.addEventListener('click', () => {
      document.querySelectorAll('.cat-pill').forEach(b => b.classList.remove('active'));
      pill.classList.add('active');
      activeCatFilter = pill.dataset.cat || 'ALL';
      renderPresetTable();
      const firstMatch = PRESET_CATALOG.find((p) => {
        if (activePackFilter !== 'ALL' && p.pack !== activePackFilter) return false;
        if (activeCatFilter === 'FAV' && !favoritePresets.has(p.id)) return false;
        if (activeCatFilter !== 'ALL' && activeCatFilter !== 'FAV' && p.cat !== activeCatFilter) return false;
        return true;
      });
      if (firstMatch) applyPresetFromCatalog(firstMatch, true);
    });
  });

  document.querySelectorAll('.pack-pill').forEach((pill) => {
    pill.addEventListener('click', () => {
      const packId = pill.dataset.pack || 'ALL';
      activePackFilter = packId;
      document.querySelectorAll('.pack-pill').forEach(b => b.classList.toggle('active', b === pill));
      const firstPackPreset = PRESET_CATALOG.find(p => packId === 'ALL' || p.pack === packId);
      if (firstPackPreset) {
        applyPresetFromCatalog(firstPackPreset, true);
      }
      setBrowserOpen(true);
    });
  });

  // Botones dedicados del módulo 6/8 Joropo Arp
  document.querySelectorAll('.arp-mode-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      const m = parseInt(btn.dataset.arp, 10) || 0;
      updateArpModeUI(m);
      sendFastArpMode(m);
    });
  });

  // Encoders cepillados de Trim Start y Trim End (Módulo 1)
  const bindTrimEncoder = (elId, isStart) => {
    const el = document.getElementById(elId);
    if (!el) return;
    let dragging = false;
    let startY = 0;
    let startVal = 0;
    el.addEventListener('mousedown', (e) => {
      dragging = true;
      startY = e.clientY;
      startVal = isStart ? state.region.trim_start_norm : state.region.trim_end_norm;
      e.preventDefault();
    });
    window.addEventListener('mousemove', (e) => {
      if (!dragging) return;
      const dy = (startY - e.clientY) * 0.004;
      if (isStart) {
        state.region.trim_start_norm = Math.max(0.0, Math.min(state.region.trim_end_norm - 0.02, startVal + dy));
      } else {
        state.region.trim_end_norm = Math.min(1.0, Math.max(state.region.trim_start_norm + 0.02, startVal + dy));
      }
      sendFastRegion();
    });
    window.addEventListener('mouseup', () => {
      dragging = false;
    });
  };
  bindTrimEncoder('enc-trim-start', true);
  bindTrimEncoder('enc-trim-end', false);
}

// -------------------------------------------------------------
// 9. INICIALIZACIÓN GLOBAL
// -------------------------------------------------------------
let appInitialized = false;

function initApp() {
  if (appInitialized) return;
  appInitialized = true;

  sendIpc('log:initApp starting...');

  // Deshabilitar menú contextual del navegador, pero permitir Shift + Clic Derecho para DevTools
  window.addEventListener('contextmenu', (e) => {
    if (!e.shiftKey) e.preventDefault();
  });

  initKnobs();
  initFilterModes();
  initOscilloscope();
  initInteractiveOleds();
  initOledThemeSwitcher();
  initKeyboard();
  initKeyboardEvents();
  initAiHub();
  initPresetBrowser();
  initWindowResizeControls();
  initAudioSettingsModal();
  fetchInitState();
  startTelemetryLoop();
  updateEngineBadge();
  hideParamHudTooltip();

  // Redimensionar piano y oscilograma en resize
  window.addEventListener('resize', () => {
    initKeyboard();
  });

  sendIpc('log:initApp finished successfully!');
}

export { initApp };

// 10. REDIMENSIONADO NATIVO Y ESCALADO RESPONSIVO (STANDALONE & DAW)
// -------------------------------------------------------------
const BASE_UI_W = 1180;
const BASE_UI_H = 780;
const UI_SCALE_STEPS = [1.0, 1.15, 1.3, 0.9];
let currentScaleIdx = 0;

function applyResponsiveViewport() {
  const container = document.querySelector('.plugin-container');
  const scaleBtn = document.getElementById('btn-ui-scale');
  if (!container) return;

  const vw = window.innerWidth || BASE_UI_W;
  const vh = window.innerHeight || BASE_UI_H;

  // Si la ventana es más pequeña que 900x640, aplicamos zoom proporcional automático para evitar cualquier solapamiento
  if (vw < 900 || vh < 640) {
    const z = Math.max(0.65, Math.min(vw / 900, vh / 640));
    container.style.zoom = String(z);
  } else {
    container.style.zoom = '1';
  }

  if (scaleBtn) {
    const pct = Math.round(((vw / BASE_UI_W) + (vh / BASE_UI_H)) * 50);
    scaleBtn.textContent = `SCALE ${pct}%`;
  }
}

function initWindowResizeControls() {
  const scaleBtn = document.getElementById('btn-ui-scale');
  const grip = document.getElementById('resize-grip');

  if (scaleBtn) {
    scaleBtn.addEventListener('click', () => {
      currentScaleIdx = (currentScaleIdx + 1) % UI_SCALE_STEPS.length;
      const scale = UI_SCALE_STEPS[currentScaleIdx];
      const targetW = Math.round(BASE_UI_W * scale);
      const targetH = Math.round(BASE_UI_H * scale);
      scaleBtn.textContent = `SCALE ${Math.round(scale * 100)}%`;
      state.localUiResizeEvents = (state.localUiResizeEvents || 0) + 1;
      sendIpc(`resize:${targetW},${targetH}`);
      refreshFooterOddText();
    });
  }

  if (grip) {
    let draggingResize = false;
    let startScreenX = 0;
    let startScreenY = 0;
    let startW = BASE_UI_W;
    let startH = BASE_UI_H;
    let rafPending = false;
    let pendingW = BASE_UI_W;
    let pendingH = BASE_UI_H;

    grip.addEventListener('mousedown', (e) => {
      draggingResize = true;
      startScreenX = e.screenX;
      startScreenY = e.screenY;
      startW = window.innerWidth || BASE_UI_W;
      startH = window.innerHeight || BASE_UI_H;
      document.body.style.cursor = 'nwse-resize';
      e.preventDefault();
      e.stopPropagation();
    });

    window.addEventListener('mousemove', (e) => {
      if (!draggingResize) return;
      const dx = e.screenX - startScreenX;
      const dy = e.screenY - startScreenY;
      pendingW = Math.max(760, Math.min(3840, Math.round(startW + dx)));
      pendingH = Math.max(540, Math.min(2160, Math.round(startH + dy)));
      if (!rafPending) {
        rafPending = true;
        requestAnimationFrame(() => {
          rafPending = false;
          sendIpc(`resize:${pendingW},${pendingH}`);
        });
      }
    });

    window.addEventListener('mouseup', () => {
      if (draggingResize) {
        draggingResize = false;
        document.body.style.cursor = 'default';
      }
    });

    grip.addEventListener('dblclick', (e) => {
      currentScaleIdx = 0;
      sendIpc(`resize:${BASE_UI_W},${BASE_UI_H}`);
      e.preventDefault();
    });
  }

  window.addEventListener('resize', () => {
    applyResponsiveViewport();
  });

  applyResponsiveViewport();
}
