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
    arp_mode: 0
  },
  normalized: {
    attack: 0.002,
    decay: 0.05,
    sustain: 0.8,
    release: 0.03,
    cutoff: 1.0,
    resonance: 0.06,
    gain: 0.909,
    space_mix: 0.15,
    root_note: 0.4724,
    lfo_rate: 0.32,
    lfo_depth: 0.0
  },
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
  state.activeNotes.add(midi);
  updateVoiceCount();
  sendIpc(`1,${midi},${velocity.toFixed(3)}`);
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
    loopBtn.innerText = r.loop_enabled ? 'LOOP: ON' : 'LOOP: OFF';
  }
}

function refreshFooterOddText() {
  const footerText = document.getElementById('footer-text');
  if (!footerText) return;
  const odd = state.oddMetrics || {};
  const backend = (odd.synth_backend || 'HYBRID-PHYSICAL').toUpperCase();
  const durUs = odd.last_generation_duration_us || 0;
  const genCnt = odd.generation_count || 1;
  const blk = odd.dsp_blocks_processed || 0;
  const lim = odd.dsp_limiter_activations || 0;
  const lfoMod = Math.round(Math.abs(odd.lfo_mod_value || 0) * 100);
  const presets = Math.max(odd.ui_preset_loads || 0, state.localUiPresetLoads || 0);
  const matrix = Math.max(odd.ui_matrix_switches || 0, state.localUiMatrixSwitches || 0);
  const resizes = Math.max(odd.ui_resize_events || 0, state.localUiResizeEvents || 0);
  footerText.innerText = `ODD // BACKEND: ${backend} • SYNTH: ${durUs}µs (#${genCnt}) • LFO: ${lfoMod}% • PRESETS: ${presets} • MATRIX: ${matrix} • RESIZE: ${resizes} • DSP BLK: ${blk} • LIM: ${lim}`;
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
    telemRootEl.innerText = `SAMPLE: ${upperName} • ROOT: ${noteLabel}`;
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
          if (data.params) Object.assign(state.params, data.params);
          if (data.normalized) Object.assign(state.normalized, data.normalized);
          if (data.param_revision !== undefined) state.lastParamRevision = data.param_revision;
          if (data.waveform && data.waveform.length > 0) {
            state.waveform = data.waveform;
          }
          if (data.sample_info) updateTelemetry(data.sample_info);
          if (data.region && !state.isDraggingRegion) updateRegionDisplay(data.region);
          if (data.odd_metrics) updateOddMetricsDisplay(data.odd_metrics);
          drawOscilloscope();
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

function applyInitPayload(msg) {
  if (!msg) return;
  if (msg.params) Object.assign(state.params, msg.params);
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
          if (data.params) Object.assign(state.params, data.params);
          if (data.normalized) Object.assign(state.normalized, data.normalized);
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

function showParamHudTooltip(anchorEl, paramName, formattedVal) {
  const hud = document.getElementById('param-hud-tooltip');
  if (!hud || !anchorEl) return;
  const rect = anchorEl.getBoundingClientRect();
  hud.innerHTML = `<span class="hud-param-name">${paramName}</span><span class="hud-param-val">${formattedVal}</span>`;
  hud.style.left = `${Math.round(rect.left + rect.width * 0.5)}px`;
  hud.style.top = `${Math.max(24, Math.round(rect.top - 6))}px`;
  hud.classList.remove('hidden');
}

function hideParamHudTooltip() {
  const hud = document.getElementById('param-hud-tooltip');
  if (hud) hud.classList.add('hidden');
}

function initKnobs() {
  document.querySelectorAll('.knob-container').forEach((container) => {
    const paramId = container.dataset.param;
    const widget = container.querySelector('.knob-widget');
    const accentClass = getAccentClass(paramId);
    const labelEl = container.querySelector('.knob-label');
    const paramTitle = (labelEl ? labelEl.innerText : paramId).toUpperCase();

    widget.innerHTML = `
      <svg class="knob-svg" viewBox="0 0 46 46">
        <circle class="knob-bg-track" cx="23" cy="23" r="18" fill="none" stroke="#161e2b" stroke-width="3.2" stroke-dasharray="84.8 113.1" stroke-dashoffset="0" stroke-linecap="round" />
        <circle class="knob-active-track ${accentClass}-track" cx="23" cy="23" r="18" fill="none" stroke="currentColor" stroke-width="3.2" stroke-dasharray="0 113.1" stroke-dashoffset="0" stroke-linecap="round" />
        <circle class="knob-mod-track" cx="23" cy="23" r="20.6" fill="none" stroke="#10b981" stroke-width="1.6" stroke-dasharray="0 129.4" stroke-dashoffset="0" stroke-linecap="round" opacity="0" />
      </svg>
      <div class="knob-cap ${accentClass}">
        <div class="knob-notch"></div>
      </div>
    `;

    // Interacción Drag vertical de alta precisión
    let isDragging = false;
    let startY = 0;
    let startNorm = 0;

    container.addEventListener('mousedown', (e) => {
      isDragging = true;
      state.isDraggingKnob = true;
      startY = e.clientY;
      startNorm = state.normalized[paramId] || 0.0;
      document.body.style.cursor = 'ns-resize';
      showParamHudTooltip(container, paramTitle, formatKnobValue(paramId, startNorm));
      e.preventDefault();
    });

    window.addEventListener('mousemove', (e) => {
      if (!isDragging) return;
      const deltaY = startY - e.clientY;
      // Shift para precisión milimétrica (10x más fino)
      const step = e.shiftKey ? 0.001 : 0.005;
      const newNorm = Math.max(0.0, Math.min(1.0, startNorm + deltaY * step));
      
      state.normalized[paramId] = newNorm;
      updateKnobUI(paramId, newNorm);
      showParamHudTooltip(container, paramTitle, formatKnobValue(paramId, newNorm));
      sendFastParam(paramId, newNorm);
    });

    window.addEventListener('mouseup', () => {
      if (isDragging) {
        isDragging = false;
        state.isDraggingKnob = false;
        document.body.style.cursor = 'default';
        hideParamHudTooltip();
      }
    });

    // Rueda del ratón
    container.addEventListener('wheel', (e) => {
      e.preventDefault();
      const current = state.normalized[paramId] || 0.0;
      const step = (e.deltaY < 0 ? 1 : -1) * (e.shiftKey ? 0.005 : 0.025);
      const newNorm = Math.max(0.0, Math.min(1.0, current + step));
      
      state.normalized[paramId] = newNorm;
      updateKnobUI(paramId, newNorm);
      sendFastParam(paramId, newNorm);
    }, { passive: false });

    // Doble clic: reiniciar a defecto de fábrica
    container.addEventListener('dblclick', () => {
      let defNorm = 0.5;
      if (paramId === 'cutoff') defNorm = 1.0;
      if (paramId === 'resonance') defNorm = 0.061;
      if (paramId === 'gain') defNorm = 0.909;
      if (paramId === 'space_mix') defNorm = 0.15;
      if (paramId === 'lfo_rate') defNorm = 0.32;
      if (paramId === 'lfo_depth') defNorm = 0.0;
      if (paramId === 'attack') defNorm = 0.002;
      if (paramId === 'decay') defNorm = 0.05;
      if (paramId === 'sustain') defNorm = 0.8;
      if (paramId === 'release') defNorm = 0.03;
      
      state.normalized[paramId] = defNorm;
      updateKnobUI(paramId, defNorm);
      sendFastParam(paramId, defNorm);
    });
  });
}

function getAccentClass(id) {
  if (['attack', 'decay', 'sustain', 'release'].includes(id)) return 'cyan-accent';
  if (['cutoff', 'resonance'].includes(id)) return 'amber-accent';
  return 'green-accent';
}

function getAccentColor(id) {
  if (['attack', 'decay', 'sustain', 'release'].includes(id)) return '#00ebff';
  if (['cutoff', 'resonance'].includes(id)) return '#ffaa00';
  return '#10b981';
}

function updateKnobUI(paramId, norm, displayVal) {
  const container = document.querySelector(`.knob-container[data-param="${paramId}"]`);
  if (!container) return;

  const cap = container.querySelector('.knob-cap');
  const activeTrack = container.querySelector('.knob-active-track');
  const valLabel = container.querySelector('.knob-val');

  // Rotar el cap de aluminio (-135deg a +135deg)
  const angle = -135 + norm * 270;
  if (cap) cap.style.transform = `rotate(${angle}deg)`;

  // Actualizar arco SVG (longitud total del arco de 270° es ~84.82)
  const maxArc = (270 / 360) * KNOB_CONFIG.circumference;
  const currentArc = norm * maxArc;
  if (activeTrack) {
    activeTrack.setAttribute('stroke-dasharray', `${currentArc} ${KNOB_CONFIG.circumference}`);
    const color = getAccentColor(paramId);
    activeTrack.setAttribute('stroke', color);
  }

  // Actualizar Fader vertical hardware si existe (Módulo 3 ADSR Fader Bank)
  const faderFill = document.getElementById(`fader-fill-${paramId}`);
  const faderThumb = document.getElementById(`fader-thumb-${paramId}`);
  const pct = Math.max(0, Math.min(100, norm * 100));
  if (faderFill) faderFill.style.height = `${pct.toFixed(1)}%`;
  if (faderThumb) faderThumb.style.bottom = `${pct.toFixed(1)}%`;

  // Actualizar texto de valor
  if (valLabel) {
    if (displayVal !== undefined) {
      valLabel.innerText = displayVal;
    } else {
      valLabel.innerText = formatKnobValue(paramId, norm);
    }
  }
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
  if (id === 'space_mix' || id === 'lfo_depth') {
    return `${Math.round(norm * 100)} %`;
  }
  if (id === 'lfo_rate') {
    const val = 0.1 * Math.pow(20.0 / 0.1, norm);
    return `${val.toFixed(1)} Hz`;
  }
  return norm.toFixed(2);
}

const LFO_TARGET_LABELS = ['LFO → CUTOFF', 'LFO → VIBRATO', 'LFO → TREMOLO', 'LFO → OFF'];

function updateLfoTargetUI(targetInt) {
  const t = Math.max(0, Math.min(3, Math.round(Number(targetInt) || 0)));
  state.params.lfo_target = t;
  const btn = document.getElementById('btn-lfo-target');
  if (btn) {
    btn.innerText = LFO_TARGET_LABELS[t] || 'LFO → CUTOFF';
  }
}

function updateAllKnobsUI() {
  for (const [id, norm] of Object.entries(state.normalized)) {
    updateKnobUI(id, norm);
  }
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
// 5. FILTER MODE SWITCHER
// -------------------------------------------------------------
function initFilterModes() {
  document.querySelectorAll('.mode-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      const mode = parseInt(btn.dataset.mode, 10);
      updateFilterModeUI(mode);
      sendFastFilterMode(mode);
    });
  });
}

function updateFilterModeUI(mode) {
  document.querySelectorAll('.mode-btn').forEach((btn) => {
    const btnMode = parseInt(btn.dataset.mode, 10);
    btn.classList.toggle('active', btnMode === mode);
  });
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

    canvas.addEventListener('mousedown', (e) => {
      const rect = canvas.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;
      const normX = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
      const normY = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));
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

      if (!activeRegionMarker) {
        if (e.target === canvas) {
          const hover = pickMarker(normX, normY);
          canvas.style.cursor = hover ? 'ew-resize' : 'crosshair';
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

function startVisualizationLoop() {
  if (animFrameId) cancelAnimationFrame(animFrameId);

  const render = () => {
    drawOscilloscope();
    drawSvfOled();
    drawAdsrOled();
    drawLfoOled();
    drawArpOled();
    drawReverbOled();
    updateVuMeters();
    updateLiveKnobModulationRings();
    animPhase += 0.05;
    animFrameId = requestAnimationFrame(render);
  };

  animFrameId = requestAnimationFrame(render);
}

function sizeOledCanvas(canvas) {
  const rect = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const targetW = Math.round(rect.width * dpr);
  const targetH = Math.round(rect.height * dpr);
  if (targetW > 0 && targetH > 0 && (canvas.width !== targetW || canvas.height !== targetH)) {
    canvas.width = targetW;
    canvas.height = targetH;
  }
  return dpr;
}

function drawSvfOled() {
  const canvas = document.getElementById('oled-svf');
  if (!canvas) return;
  const dpr = sizeOledCanvas(canvas);
  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;
  if (w <= 0 || h <= 0) return;

  ctx.fillStyle = '#03070b';
  ctx.fillRect(0, 0, w, h);

  // Rejilla logarítmica de frecuencias
  ctx.strokeStyle = '#0c1622';
  ctx.lineWidth = 1;
  for (let i = 1; i < 6; i++) {
    const x = (w / 6) * i;
    ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, h); ctx.stroke();
  }
  ctx.beginPath(); ctx.moveTo(0, h * 0.55); ctx.lineTo(w, h * 0.55); ctx.stroke();

  const cutNorm = state.normalized.cutoff !== undefined ? state.normalized.cutoff : 1.0;
  const resNorm = state.normalized.resonance !== undefined ? state.normalized.resonance : 0.06;
  const mode = state.params.filter_mode || 0;
  const lfoDepth = (state.params.lfo_target === 0) ? (state.params.lfo_depth || 0) : 0;
  const modOffset = lfoDepth * Math.sin(animPhase) * 0.16;
  const effCut = Math.max(0.06, Math.min(0.94, cutNorm + modOffset));
  const peakBoost = resNorm * 0.44;

  const evalCurve = (xNorm, cPos, m) => {
    const d = (xNorm - cPos) * 7.5;
    const resPeak = peakBoost * Math.exp(-(d * d) * 3.8);
    if (m === 3) return 0.52; // Bypass
    if (m === 0) { // Lowpass
      const base = 1.0 / (1.0 + Math.exp(d * 2.4));
      return Math.min(0.95, base * 0.55 + resPeak);
    }
    if (m === 1) { // Highpass
      const base = 1.0 / (1.0 + Math.exp(-d * 2.4));
      return Math.min(0.95, base * 0.55 + resPeak);
    }
    // Bandpass
    return Math.min(0.95, Math.exp(-(d * d) * 1.1) * (0.55 + resPeak * 0.7));
  };

  // Relleno cian bajo la curva
  const grad = ctx.createLinearGradient(0, 0, 0, h);
  grad.addColorStop(0, 'rgba(0, 235, 255, 0.34)');
  grad.addColorStop(1, 'rgba(0, 235, 255, 0.02)');

  ctx.beginPath();
  ctx.moveTo(0, h - 4);
  for (let px = 0; px <= w; px += 2) {
    const xn = px / w;
    const yn = evalCurve(xn, effCut, mode);
    const y = (h - 6) - yn * (h - 14);
    ctx.lineTo(px, y);
  }
  ctx.lineTo(w, h - 4);
  ctx.closePath();
  ctx.fillStyle = grad;
  ctx.fill();

  // Trazo principal de la respuesta SVF
  ctx.beginPath();
  for (let px = 0; px <= w; px += 2) {
    const xn = px / w;
    const yn = evalCurve(xn, effCut, mode);
    const y = (h - 6) - yn * (h - 14);
    if (px === 0) ctx.moveTo(px, y);
    else ctx.lineTo(px, y);
  }
  ctx.strokeStyle = '#00ebff';
  ctx.lineWidth = 1.8 * dpr;
  ctx.shadowColor = '#00ebff';
  ctx.shadowBlur = 6 * dpr;
  ctx.stroke();
  ctx.shadowBlur = 0;
}

function drawAdsrOled() {
  const canvas = document.getElementById('oled-adsr');
  if (!canvas) return;
  const dpr = sizeOledCanvas(canvas);
  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;
  if (w <= 0 || h <= 0) return;

  ctx.fillStyle = '#070502';
  ctx.fillRect(0, 0, w, h);

  ctx.strokeStyle = '#1d1508';
  ctx.lineWidth = 1;
  for (let i = 1; i < 5; i++) {
    const x = (w / 5) * i;
    ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, h); ctx.stroke();
  }

  const a = Math.max(0.04, Math.min(0.32, (state.normalized.attack || 0.02) * 0.30 + 0.05));
  const d = Math.max(0.06, Math.min(0.28, (state.normalized.decay || 0.15) * 0.25 + 0.07));
  const s = Math.max(0.05, Math.min(0.95, state.normalized.sustain !== undefined ? state.normalized.sustain : 0.8));
  const r = Math.max(0.06, Math.min(0.32, (state.normalized.release || 0.15) * 0.28 + 0.07));

  const padX = 8 * dpr;
  const topY = 10 * dpr;
  const botY = h - 8 * dpr;
  const spanW = w - padX * 2;

  const x0 = padX;
  const x1 = x0 + a * spanW;
  const x2 = x1 + d * spanW;
  const x3 = x2 + 0.22 * spanW;
  const x4 = Math.min(w - padX, x3 + r * spanW);

  const ySus = botY - s * (botY - topY);

  const grad = ctx.createLinearGradient(0, topY, 0, botY);
  grad.addColorStop(0, 'rgba(255, 170, 0, 0.32)');
  grad.addColorStop(1, 'rgba(255, 170, 0, 0.02)');

  ctx.beginPath();
  ctx.moveTo(x0, botY);
  ctx.lineTo(x1, topY);
  ctx.quadraticCurveTo((x1 + x2) * 0.5, ySus, x2, ySus);
  ctx.lineTo(x3, ySus);
  ctx.quadraticCurveTo((x3 + x4) * 0.5, botY, x4, botY);
  ctx.closePath();
  ctx.fillStyle = grad;
  ctx.fill();

  ctx.beginPath();
  ctx.moveTo(x0, botY);
  ctx.lineTo(x1, topY);
  ctx.quadraticCurveTo((x1 + x2) * 0.5, ySus, x2, ySus);
  ctx.lineTo(x3, ySus);
  ctx.quadraticCurveTo((x3 + x4) * 0.5, botY, x4, botY);
  ctx.strokeStyle = '#ffaa00';
  ctx.lineWidth = 1.9 * dpr;
  ctx.shadowColor = '#ffaa00';
  ctx.shadowBlur = 6 * dpr;
  ctx.stroke();
  ctx.shadowBlur = 0;

  // Nodos de control brillantes
  [[x1, topY], [x2, ySus], [x3, ySus], [x4, botY]].forEach(([nx, ny]) => {
    ctx.fillStyle = '#ffd166';
    ctx.beginPath();
    ctx.arc(nx, ny, 2.6 * dpr, 0, Math.PI * 2);
    ctx.fill();
  });
}

function drawLfoOled() {
  const canvas = document.getElementById('oled-lfo');
  if (!canvas) return;
  const dpr = sizeOledCanvas(canvas);
  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;
  if (w <= 0 || h <= 0) return;

  ctx.fillStyle = '#020806';
  ctx.fillRect(0, 0, w, h);

  const midY = h * 0.5;
  ctx.strokeStyle = '#0b2219';
  ctx.beginPath(); ctx.moveTo(0, midY); ctx.lineTo(w, midY); ctx.stroke();

  const rateNorm = state.normalized.lfo_rate !== undefined ? state.normalized.lfo_rate : 0.32;
  const depthNorm = state.normalized.lfo_depth !== undefined ? state.normalized.lfo_depth : 0.0;
  const cycles = 1.5 + rateNorm * 4.5;
  const amp = Math.max(0.18, depthNorm) * (h * 0.38);

  const grad = ctx.createLinearGradient(0, 0, 0, h);
  grad.addColorStop(0, 'rgba(16, 185, 129, 0.25)');
  grad.addColorStop(0.5, 'rgba(16, 185, 129, 0.04)');
  grad.addColorStop(1, 'rgba(16, 185, 129, 0.25)');

  ctx.beginPath();
  ctx.moveTo(0, midY);
  for (let x = 0; x <= w; x += 2) {
    const t = x / w;
    const y = midY - Math.sin(t * Math.PI * 2 * cycles + animPhase) * amp;
    ctx.lineTo(x, y);
  }
  ctx.lineTo(w, midY);
  ctx.fillStyle = grad;
  ctx.fill();

  ctx.beginPath();
  for (let x = 0; x <= w; x += 2) {
    const t = x / w;
    const y = midY - Math.sin(t * Math.PI * 2 * cycles + animPhase) * amp;
    if (x === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.strokeStyle = '#10b981';
  ctx.lineWidth = 1.8 * dpr;
  ctx.shadowColor = '#10b981';
  ctx.shadowBlur = 6 * dpr;
  ctx.stroke();
  ctx.shadowBlur = 0;
}

function drawArpOled() {
  const canvas = document.getElementById('oled-arp');
  if (!canvas) return;
  const dpr = sizeOledCanvas(canvas);
  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;
  if (w <= 0 || h <= 0) return;

  ctx.fillStyle = '#080502';
  ctx.fillRect(0, 0, w, h);

  const mode = state.params.arp_mode || 0;
  const patterns = {
    0: [0.35, 0.35, 0.35, 0.35, 0.35, 0.35, 0.35, 0.35, 0.35, 0.35, 0.35, 0.35],
    1: [0.25, 0.40, 0.55, 0.70, 0.85, 1.00, 0.25, 0.40, 0.55, 0.70, 0.85, 1.00],
    2: [1.00, 0.85, 0.70, 0.55, 0.40, 0.25, 1.00, 0.85, 0.70, 0.55, 0.40, 0.25],
    3: [0.95, 0.48, 0.78, 0.92, 0.52, 0.86, 0.95, 0.48, 0.78, 0.92, 0.52, 0.86], // 6/8 Joropo
    4: [0.96, 0.55, 0.82, 0.48, 0.90, 0.96, 0.55, 0.82, 0.48, 0.90, 0.75, 0.60], // 5/8 Merengue
  };
  const bars = patterns[mode] || patterns[3];
  const isPlaying = state.activeNotes.size > 0 || (state.dspActiveVoices || 0) > 0;
  const activeStep = isPlaying ? Math.floor(animPhase * 2.2) % bars.length : -1;

  // Header text inside OLED
  ctx.fillStyle = '#ffaa00';
  ctx.font = `bold ${Math.round(8.5 * dpr)}px monospace`;
  const titleMap = ['4/4 MANUAL', 'ARP UP', 'ARP DOWN', '6/8 JOROPO', '5/8 MERENGUE'];
  ctx.fillText(titleMap[mode] || '6/8 JOROPO', 6 * dpr, 11 * dpr);
  ctx.fillText(mode === 4 ? '5/8' : (mode === 3 ? '6/8' : '3/4'), w - 24 * dpr, 11 * dpr);

  const numBars = bars.length;
  const padX = 8 * dpr;
  const availW = w - padX * 2;
  const slotW = availW / numBars;
  const barW = Math.max(4 * dpr, slotW * 0.68);
  const botY = h - 12 * dpr;
  const maxBarH = h - 28 * dpr;

  for (let i = 0; i < numBars; i++) {
    const bh = bars[i] * maxBarH;
    const bx = padX + i * slotW + (slotW - barW) * 0.5;
    const by = botY - bh;
    const isHit = i === activeStep;

    ctx.fillStyle = isHit ? '#ffe082' : (i % 3 === 0 ? '#ffaa00' : 'rgba(255, 170, 0, 0.62)');
    ctx.fillRect(bx, by, barW, bh);

    ctx.fillStyle = '#a16207';
    ctx.font = `${Math.round(6.5 * dpr)}px monospace`;
    ctx.fillText(String((i % 9) + 1), bx + 1 * dpr, h - 3 * dpr);
  }
}

function drawReverbOled() {
  const canvas = document.getElementById('oled-reverb');
  if (!canvas) return;
  const dpr = sizeOledCanvas(canvas);
  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;
  if (w <= 0 || h <= 0) return;

  ctx.fillStyle = '#03070b';
  ctx.fillRect(0, 0, w, h);

  const midY = h * 0.5;
  const mix = Math.max(0.15, state.normalized.space_mix !== undefined ? state.normalized.space_mix : 0.15);
  const isPlaying = state.activeNotes.size > 0 || (state.dspActiveVoices || 0) > 0;

  ctx.strokeStyle = '#0e1d2b';
  ctx.beginPath(); ctx.moveTo(0, midY); ctx.lineTo(w, midY); ctx.stroke();

  ctx.beginPath();
  for (let x = 0; x < w; x++) {
    const t = x / w;
    const env = Math.exp(-t * (4.2 - mix * 2.6));
    const tail = Math.sin(x * 0.65 + (isPlaying ? animPhase * 3 : 0)) * Math.cos(x * 0.23) * env;
    const y = midY - tail * (h * 0.42) * (0.45 + mix * 0.55);
    ctx.moveTo(x, midY - Math.abs(y - midY));
    ctx.lineTo(x, midY + Math.abs(y - midY));
  }
  ctx.strokeStyle = 'rgba(0, 235, 255, 0.72)';
  ctx.lineWidth = 1;
  ctx.stroke();
}

function drawOscilloscope() {
  const canvas = document.getElementById('oscilloscope');
  if (!canvas) return;

  // HiDPI auto-scaler para máxima nitidez OLED en pantallas 4K y escaladas
  const rect = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const targetW = Math.round(rect.width * dpr);
  const targetH = Math.round(rect.height * dpr);
  if (targetW > 0 && targetH > 0 && (canvas.width !== targetW || canvas.height !== targetH)) {
    canvas.width = targetW;
    canvas.height = targetH;
  }

  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;

  // Fondo OLED ultra negro
  ctx.fillStyle = '#040609';
  ctx.fillRect(0, 0, w, h);

  const centerY = h / 2;

  // Rejilla técnica de osciloscopio
  ctx.strokeStyle = '#0e141f';
  ctx.lineWidth = 1;

  // Línea cero central
  ctx.beginPath();
  ctx.moveTo(0, centerY);
  ctx.lineTo(w, centerY);
  ctx.strokeStyle = '#182232';
  ctx.stroke();

  // Líneas de división verticales (12 divisiones)
  ctx.strokeStyle = '#0c121b';
  for (let i = 1; i < 12; i++) {
    const x = (w / 12) * i;
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, h);
    ctx.stroke();
  }

  // Silueta topográfica del Tepui Roraima en el sector derecho del OLED (Concept 2 Signature)
  ctx.save();
  const tepuiPts = [
    [0.44, 0.88], [0.50, 0.82], [0.55, 0.70], [0.58, 0.56],
    [0.61, 0.25], [0.64, 0.22], [0.71, 0.23], [0.76, 0.21],
    [0.82, 0.23], [0.85, 0.26], [0.88, 0.56], [0.93, 0.74], [0.98, 0.88]
  ];
  ctx.beginPath();
  tepuiPts.forEach(([tx, ty], idx) => {
    const px = tx * w;
    const py = ty * h;
    if (idx === 0) ctx.moveTo(px, py);
    else ctx.lineTo(px, py);
  });
  ctx.strokeStyle = 'rgba(0, 235, 255, 0.55)';
  ctx.lineWidth = 1.4 * dpr;
  ctx.stroke();
  ctx.lineTo(0.98 * w, h);
  ctx.lineTo(0.44 * w, h);
  ctx.closePath();
  ctx.fillStyle = 'rgba(0, 235, 255, 0.06)';
  ctx.fill();
  ctx.restore();

  // Trazar forma de onda
  const pts = state.waveform;
  if (!pts || pts.length < 2) return;

  const isPlaying = state.activeNotes.size > 0 || (state.dspActiveVoices || 0) > 0;
  const wobble = isPlaying ? Math.sin(animPhase) * 0.08 : 0;

  // Relleno gradiente con glow cian
  const grad = ctx.createLinearGradient(0, 0, 0, h);
  grad.addColorStop(0, 'rgba(0, 235, 255, 0.22)');
  grad.addColorStop(0.5, 'rgba(0, 235, 255, 0.04)');
  grad.addColorStop(1, 'rgba(0, 235, 255, 0.22)');

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

  // Línea de señal de fósforo cian brillante con resplandor
  ctx.beginPath();
  for (let i = 0; i < pts.length; i++) {
    const x = (i / (pts.length - 1)) * w;
    const sampleVal = pts[i] * (1.0 + wobble);
    const y = centerY - sampleVal * (centerY - 10);
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.strokeStyle = isPlaying ? '#e0ffff' : '#00ebff';
  ctx.lineWidth = isPlaying ? 2.0 : 1.6;
  ctx.shadowColor = '#00ebff';
  ctx.shadowBlur = isPlaying ? 10 : 5;
  ctx.stroke();
  ctx.shadowBlur = 0;

  // --- OVERLAY INTERACTIVO DE TRIM (START / END) Y LOOP (L1 / L2) ---
  const r = state.region || { trim_start_norm: 0, trim_end_norm: 1, loop_enabled: true, loop_start_norm: 0.2, loop_end_norm: 0.85 };
  const trimStartX = Math.round(r.trim_start_norm * w);
  const trimEndX = Math.round(r.trim_end_norm * w);
  const loopStartX = Math.round(r.loop_start_norm * w);
  const loopEndX = Math.round(r.loop_end_norm * w);

  // Sombreado de zonas recortadas (fuera de Trim Start .. Trim End)
  ctx.fillStyle = 'rgba(2, 4, 8, 0.72)';
  if (trimStartX > 0) {
    ctx.fillRect(0, 0, trimStartX, h);
  }
  if (trimEndX < w) {
    ctx.fillRect(trimEndX, 0, w - trimEndX, h);
  }

  // Región de Loop Sostenido (cuando loop_enabled = true)
  if (r.loop_enabled && loopEndX > loopStartX) {
    ctx.fillStyle = 'rgba(255, 170, 0, 0.11)';
    ctx.fillRect(loopStartX, 0, loopEndX - loopStartX, h);

    ctx.save();
    ctx.strokeStyle = '#ffaa00';
    ctx.lineWidth = Math.max(1.2, 1.2 * dpr);
    ctx.setLineDash([4 * dpr, 3 * dpr]);
    ctx.beginPath();
    ctx.moveTo(loopStartX, 0);
    ctx.lineTo(loopStartX, h);
    ctx.moveTo(loopEndX, 0);
    ctx.lineTo(loopEndX, h);
    ctx.stroke();
    ctx.restore();

    // Banderines inferiores L1 / L2
    ctx.fillStyle = '#ffaa00';
    const flagW = 14 * dpr;
    const flagH = 11 * dpr;
    ctx.fillRect(loopStartX, h - flagH, flagW, flagH);
    ctx.fillRect(Math.max(0, loopEndX - flagW), h - flagH, flagW, flagH);
    ctx.fillStyle = '#05070b';
    ctx.font = `bold ${Math.round(7.5 * dpr)}px monospace`;
    ctx.fillText('L1', loopStartX + 2 * dpr, h - 2.5 * dpr);
    ctx.fillText('L2', Math.max(0, loopEndX - flagW) + 2 * dpr, h - 2.5 * dpr);
  }

  // Marcadores verticales de Trim Start (S) y Trim End (E)
  ctx.strokeStyle = '#10b981';
  ctx.lineWidth = Math.max(1.5, 1.5 * dpr);
  ctx.beginPath();
  ctx.moveTo(trimStartX, 0);
  ctx.lineTo(trimStartX, h);
  ctx.moveTo(trimEndX, 0);
  ctx.lineTo(trimEndX, h);
  ctx.stroke();

  const tFlagW = 11 * dpr;
  const tFlagH = 11 * dpr;
  ctx.fillStyle = '#10b981';
  ctx.fillRect(trimStartX, 0, tFlagW, tFlagH);
  ctx.fillRect(Math.max(0, trimEndX - tFlagW), 0, tFlagW, tFlagH);
  ctx.fillStyle = '#04120c';
  ctx.font = `bold ${Math.round(7.5 * dpr)}px monospace`;
  ctx.fillText('S', trimStartX + 2.5 * dpr, 8.5 * dpr);
  ctx.fillText('E', Math.max(0, trimEndX - tFlagW) + 2.5 * dpr, 8.5 * dpr);

  // --- CURVA DE MODULACIÓN LFO EN TIEMPO REAL ---
  const lfoDepth = state.params.lfo_depth || 0;
  const lfoTarget = state.params.lfo_target !== undefined ? state.params.lfo_target : 0;
  if (lfoDepth > 0.005 && lfoTarget !== 3) {
    const lfoRate = state.params.lfo_rate || 3.5;
    const cycles = Math.max(1.0, Math.min(12.0, lfoRate * 0.6));
    const lfoLive = (state.oddMetrics && typeof state.oddMetrics.lfo_mod_value === 'number')
      ? state.oddMetrics.lfo_mod_value
      : Math.sin(animPhase) * lfoDepth;

    ctx.save();
    ctx.beginPath();
    for (let x = 0; x <= w; x += Math.max(2, Math.round(2 * dpr))) {
      const normX = x / w;
      const waveY = centerY - Math.sin(normX * Math.PI * 2 * cycles + animPhase) * lfoDepth * (centerY * 0.65);
      if (x === 0) ctx.moveTo(x, waveY);
      else ctx.lineTo(x, waveY);
    }
    ctx.strokeStyle = 'rgba(16, 185, 129, 0.55)';
    ctx.lineWidth = Math.max(1.2, 1.2 * dpr);
    ctx.stroke();

    // Indicador vertical lateral de modulación LFO instantánea
    const modY = centerY - lfoLive * (centerY * 0.65);
    ctx.fillStyle = '#10b981';
    ctx.beginPath();
    ctx.arc(w - 6 * dpr, modY, 3 * dpr, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  // --- CABEZAL DE REPRODUCCIÓN EN VIVO (PLAYHEAD CURSOR) ---
  const phNorm = state.playheadNorm || 0;
  if (isPlaying || phNorm > 0.001) {
    const effPlayhead = phNorm > 0.001
      ? phNorm
      : (r.trim_start_norm + ((animPhase * 0.08) % Math.max(0.05, r.trim_end_norm - r.trim_start_norm)));
    const phX = Math.round(Math.max(0, Math.min(1, effPlayhead)) * w);

    ctx.save();
    ctx.strokeStyle = '#ffffff';
    ctx.lineWidth = Math.max(1.6, 1.6 * dpr);
    ctx.shadowColor = '#00ebff';
    ctx.shadowBlur = 8 * dpr;
    ctx.beginPath();
    ctx.moveTo(phX, 0);
    ctx.lineTo(phX, h);
    ctx.stroke();

    // Triángulo superior del Playhead
    ctx.fillStyle = '#00ebff';
    ctx.beginPath();
    ctx.moveTo(phX - 4 * dpr, 0);
    ctx.lineTo(phX + 4 * dpr, 0);
    ctx.lineTo(phX, 6 * dpr);
    ctx.closePath();
    ctx.fill();
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

  const fillL = document.getElementById('meter-fill-l');
  const fillR = document.getElementById('meter-fill-r');

  if (fillL) fillL.style.transform = `scaleY(${state.vuLevelL.toFixed(3)})`;
  if (fillR) fillR.style.transform = `scaleY(${state.vuLevelR.toFixed(3)})`;
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
    if (durEl) durEl.innerText = `DUR: ${info.duration_secs.toFixed(2)}s`;
  }
  if (info.num_samples !== undefined) {
    const sampEl = document.getElementById('telemetry-samples');
    if (sampEl) sampEl.innerText = `SAMPLES: ${info.num_samples.toLocaleString()}`;
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
  const piano = document.getElementById('piano-keyboard');
  if (!piano) return;
  piano.innerHTML = '';

  const baseNote = getBaseMidiNote();
  const totalWhite = WHITE_KEY_DEFS.length;

  // 0. Tira LED Tricolor por Zonas (Low=Cyan, Mid=Green, High=Amber)
  const kzStrip = document.getElementById('keyzone-led-strip');
  if (kzStrip) {
    kzStrip.innerHTML = '';
    WHITE_KEY_DEFS.forEach((k, idx) => {
      const midi = baseNote + k.semi;
      const led = document.createElement('i');
      if (midi <= 55 || idx < 5) led.className = 'kz-low';
      else if (midi >= 77 || idx >= 10) led.className = 'kz-high';
      else led.className = 'kz-mid';
      kzStrip.appendChild(led);
    });
  }

  // 1. Teclas blancas
  WHITE_KEY_DEFS.forEach((k) => {
    const midi = baseNote + k.semi;
    const octOffset = Math.floor(k.semi / 12);
    const octNumber = state.currentOctave + octOffset;
    const label = `${k.note}${octNumber}`;

    const keyEl = document.createElement('div');
    keyEl.className = 'piano-key white-key';
    keyEl.dataset.midi = midi;

    keyEl.innerHTML = `
      <span class="key-note">${label}</span>
      ${k.shortcut ? `<span class="key-shortcut">[${k.shortcut}]</span>` : ''}
    `;

    piano.appendChild(keyEl);
  });

  // 2. Teclas negras con posicionamiento proporcional
  requestAnimationFrame(() => {
    const pianoRect = piano.getBoundingClientRect();
    const whiteW = pianoRect.width / totalWhite;

    BLACK_KEY_DEFS.forEach((bk) => {
      const midi = baseNote + bk.semi;
      const octOffset = Math.floor(bk.semi / 12);
      const octNumber = state.currentOctave + octOffset;
      const label = `${bk.note}${octNumber}`;

      const blackEl = document.createElement('div');
      blackEl.className = 'piano-key black-key';
      blackEl.dataset.midi = midi;

      const leftPos = (bk.afterIndex + 1) * whiteW - (whiteW * 0.58) / 2;
      blackEl.style.left = `${leftPos}px`;
      blackEl.style.width = `${whiteW * 0.58}px`;

      blackEl.innerHTML = `
        <span class="key-note">${label}</span>
        ${bk.shortcut ? `<span class="key-shortcut">[${bk.shortcut}]</span>` : ''}
      `;

      piano.appendChild(blackEl);
    });
  });

  // Actualizar badge de octava en barra de herramientas
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

  const arpBtn = document.getElementById('btn-arp-mode');
  if (arpBtn) {
    arpBtn.addEventListener('click', () => {
      const nextMode = ((state.params.arp_mode || 0) + 1) % 5;
      state.params.arp_mode = nextMode;
      updateArpModeUI(nextMode);
      sendIpc(`arp,${nextMode}`);
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
  'JOROPO 6/8',
  'MERENGUE 5/8'
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
    const meters = ['4/4 FREE', '4/4 UP', '4/4 DOWN', '6/8 JOROPO', '5/8 MERENGUE'];
    meterLbl.innerText = meters[m] || '4/4 FREE';
  }
}

function handlePointerNote(e) {
  const target = document.elementFromPoint(e.clientX, e.clientY);
  if (!target) return;
  const keyEl = target.closest('.piano-key');

  if (keyEl && keyEl.dataset.midi) {
    const note = parseInt(keyEl.dataset.midi, 10);
    // Calcular velocidad interactiva según la altura del clic en la tecla (Top=0.5, Bottom=1.0)
    const rect = keyEl.getBoundingClientRect();
    const relY = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));
    const vel = 0.5 + relY * 0.5;

    if (note !== activeMouseNote) {
      if (activeMouseNote !== null) releaseNote(activeMouseNote);
      activeMouseNote = note;
      pressNote(note, vel);
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
}

function releaseNote(midi) {
  const el = document.querySelector(`.piano-key[data-midi="${midi}"]`);
  if (el) el.classList.remove('active');
  sendFastNoteOff(midi);
}

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
  const newVel = Math.max(0.1, Math.min(1.0, state.currentVelocity + delta));
  state.currentVelocity = newVel;
  const badge = document.getElementById('vel-badge');
  if (badge) badge.innerText = `${Math.round(newVel * 100)}%`;
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
  updateVoiceCount();
}

// -------------------------------------------------------------
// 8. AI GENERATION HUB, PRESET CHIPS, WAV IMPORT/EXPORT & ROOT NOTE
// -------------------------------------------------------------
async function uploadWavBytes(arrayBuffer, rawFileName) {
  try {
    const cleanName = (rawFileName || 'Imported WAV').replace(/\.wav$/i, '').trim() || 'Imported WAV';
    const resp = await fetch(`/api/load-wav?name=${encodeURIComponent(cleanName)}`, {
      method: 'POST',
      headers: { 'Content-Type': 'audio/wav' },
      body: arrayBuffer
    });
    if (resp.ok) {
      const data = await resp.json();
      document.querySelectorAll('.chip').forEach(c => c.classList.remove('active'));
      const promptInput = document.getElementById('prompt-input');
      if (promptInput) promptInput.value = cleanName;
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

  const zLow = document.getElementById('btn-zone-low');
  const zMid = document.getElementById('btn-zone-mid');
  const zHigh = document.getElementById('btn-zone-high');
  const zClr = document.getElementById('btn-zone-clear');

  if (zLow) zLow.addEventListener('click', () => sendIpc('z,0'));
  if (zMid) zMid.addEventListener('click', () => sendIpc('z,1'));
  if (zHigh) zHigh.addEventListener('click', () => sendIpc('z,2'));
  if (zClr) zClr.addEventListener('click', () => sendIpc('z,clear'));

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
    const applyMatrixPin = (colIdx) => {
      pins.forEach((p, idx) => p.classList.toggle('on', idx === colIdx));
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
      sendIpc(`matrix:${rowIdx},${colIdx}`);
      refreshFooterOddText();
    };

    pins.forEach((pinEl, colIdx) => {
      pinEl.addEventListener('click', () => applyMatrixPin(colIdx));
    });
    const lbl = rowEl.querySelector('.m-lbl');
    if (lbl) {
      lbl.addEventListener('click', () => applyMatrixPin(rowIdx % 5));
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

function initAiHub() {
  const btn = document.getElementById('btn-generate');
  const input = document.getElementById('prompt-input');

  btn.addEventListener('click', () => {
    if (state.isGenerating) return;
    const prompt = input.value.trim() || 'Concert Grand Piano C4';
    state.isGenerating = true;
    updateGeneratingUI(true);
    sendFastGenerate(prompt);
  });

  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      btn.click();
      input.blur(); // Regresar el foco al instrumento para tocar inmediatamente
    }
  });

  document.querySelectorAll('.chip').forEach((chip) => {
    chip.addEventListener('click', () => {
      document.querySelectorAll('.chip').forEach(c => c.classList.remove('active'));
      chip.classList.add('active');
      const prompt = chip.dataset.prompt;
      input.value = prompt;
      state.isGenerating = true;
      updateGeneratingUI(true);
      sendFastGenerate(prompt);
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

      if (data.is_standalone) {
        if (dawBanner) dawBanner.classList.add('hidden');
        if (btnSave) btnSave.disabled = false;
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

      // Populate Input Devices
      const selectIn = document.getElementById('select-input-device');
      if (selectIn) {
        selectIn.innerHTML = '<option value="">Desactivado (Recomendado)</option>';
        if (data.input_devices) {
          data.input_devices.forEach((dev) => {
            const opt = document.createElement('option');
            opt.value = dev.name;
            opt.innerText = `${dev.name} (${dev.channels}ch)`;
            if (data.active_input_device && data.active_input_device === dev.name) {
              opt.selected = true;
            }
            selectIn.appendChild(opt);
          });
        }
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
          input_device: selectIn && selectIn.value ? selectIn.value : null,
          sample_rate: selectSr ? parseInt(selectSr.value, 10) : 48000,
          period_size: selectBuf ? parseInt(selectBuf.value, 10) : 1056,
          midi_input: selectMidi && selectMidi.value ? selectMidi.value : null,
        };
      }

      function autoSaveConfig() {
        const cfg = getModalConfig();
        if (window.__TEPUI_AUDIO_STATUS__) {
          window.__TEPUI_AUDIO_STATUS__.active_output_device = cfg.output_device;
          window.__TEPUI_AUDIO_STATUS__.active_input_device = cfg.input_device;
          window.__TEPUI_AUDIO_STATUS__.active_sample_rate = cfg.sample_rate;
          window.__TEPUI_AUDIO_STATUS__.active_period_size = cfg.period_size;
          window.__TEPUI_AUDIO_STATUS__.active_midi_input = cfg.midi_input;
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
        selectMidi.onchange = autoSaveConfig;
      }
    } catch (e) {
      console.warn('[TEPUI AUDIO SETTINGS FETCH ERROR]', e);
      showStatus('No se pudieron leer los dispositivos: ' + e.message, true);
    }
  }

  btnOpen.addEventListener('click', () => {
    hideStatus();
    modal.classList.remove('hidden');
    loadAudioDevices(true);
  });

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
      };

      showStatus('Guardando configuración de audio...');

      if (window.__TEPUI_AUDIO_STATUS__) {
        window.__TEPUI_AUDIO_STATUS__.active_output_device = config.output_device;
        window.__TEPUI_AUDIO_STATUS__.active_input_device = config.input_device;
        window.__TEPUI_AUDIO_STATUS__.active_sample_rate = config.sample_rate;
        window.__TEPUI_AUDIO_STATUS__.active_period_size = config.period_size;
        window.__TEPUI_AUDIO_STATUS__.active_midi_input = config.midi_input;
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
  { id: 'pack-01', code: 'Pack 01:', title: 'Llanos & Joropo', icon: '<svg class="hw-svg" viewBox="0 0 16 16"><path d="M3 2v12M7 3v10M11 4v8M14 6v4"/></svg>', bg: 'linear-gradient(135deg,#f59e0b,#78350f)' },
  { id: 'pack-02', code: 'Pack 02:', title: 'Costa & Tambores', icon: '<svg class="hw-svg" viewBox="0 0 16 16"><ellipse cx="8" cy="5" rx="5" ry="2.2"/><path d="M3 5v6c0 1.2 2.2 2.2 5 2.2s5-1 5-2.2V5"/></svg>', bg: 'linear-gradient(135deg,#06b6d4,#164e63)' },
  { id: 'pack-03', code: 'Pack 03:', title: 'Gran Sabana Ambient', icon: '<svg class="hw-svg" viewBox="0 0 16 16"><path d="M1 12l3-6h8l3 6H1zM4 6l1.5-2.5h5L12 6"/></svg>', bg: 'linear-gradient(135deg,#10b981,#064e3b)' },
  { id: 'pack-04', code: 'Pack 04:', title: 'Cyber-Criollo Synthwave', icon: '<svg class="hw-svg" viewBox="0 0 16 16"><path d="M1 10l3.5-5v5l3.5-5v5l3.5-5v5L15 5"/></svg>', bg: 'linear-gradient(135deg,#00ebff,#1e3a8a)' },
  { id: 'pack-05', code: 'Pack 05:', title: 'Sub-Bass & Furruco', icon: '<svg class="hw-svg" viewBox="0 0 16 16"><path d="M1 8c2-5 3.5-5 5.5 0s3.5 5 5.5 0 2-3 3-3"/></svg>', bg: 'linear-gradient(135deg,#e05a47,#451a03)' }
];

const MASTER_PRESET_SEEDS = [
  { name: 'Roraima Harp', pack: 'pack-01', cat: 'Plucks', tags: 'Plucks, Strings', rhythm: '6/8', arp: 3, prompt: 'Arpa Llanera Venezolana C4', root: 60, norm: { attack: 0.002, decay: 0.25, sustain: 0.65, release: 0.32, cutoff: 0.92, resonance: 0.14, space_mix: 0.28, lfo_rate: 0.35, lfo_depth: 0.08 } },
  { name: 'Cumaco Sub', pack: 'pack-02', cat: 'Basses', tags: 'Costa, Basses', rhythm: '5/8', arp: 4, prompt: 'Tambor Cumaco Venezolano C3', root: 48, norm: { attack: 0.001, decay: 0.18, sustain: 0.45, release: 0.22, cutoff: 0.58, resonance: 0.24, space_mix: 0.18, lfo_rate: 0.25, lfo_depth: 0.0 } },
  { name: 'Bandola Lead', pack: 'pack-01', cat: 'Leads', tags: 'Bandola, Lead', rhythm: '6/8', arp: 3, prompt: 'Bandola Llanera Venezolana A3', root: 57, norm: { attack: 0.002, decay: 0.22, sustain: 0.72, release: 0.28, cutoff: 0.88, resonance: 0.18, space_mix: 0.22, lfo_rate: 0.42, lfo_depth: 0.12 } },
  { name: 'Joropo Arp 6/8', pack: 'pack-01', cat: 'Arps 6/8', tags: 'Joropo Arp 6/8', rhythm: '6/8', arp: 3, prompt: 'Cuatro Venezolano Camburpinton C4', root: 60, norm: { attack: 0.002, decay: 0.16, sustain: 0.60, release: 0.20, cutoff: 0.90, resonance: 0.12, space_mix: 0.20, lfo_rate: 0.38, lfo_depth: 0.05 } },
  { name: 'Quitiplás Bamboo Groove', pack: 'pack-02', cat: 'Percussion', tags: 'Percussion, Barlovento', rhythm: '5/8', arp: 4, prompt: 'Quitiplas de Barlovento C4', root: 60, norm: { attack: 0.001, decay: 0.14, sustain: 0.38, release: 0.16, cutoff: 0.86, resonance: 0.22, space_mix: 0.25, lfo_rate: 0.30, lfo_depth: 0.0 } },
  { name: 'Salto Ángel Shimmer Pad', pack: 'pack-03', cat: 'Pads', tags: 'Pads, Ambient', rhythm: '4/4', arp: 0, prompt: 'Bright Crystal Shimmer Choir Pad E4', root: 64, norm: { attack: 0.42, decay: 0.55, sustain: 0.90, release: 0.65, cutoff: 0.82, resonance: 0.16, space_mix: 0.68, lfo_rate: 0.22, lfo_depth: 0.28 } },
  { name: 'Furruco 808 Zuliano', pack: 'pack-05', cat: 'Basses', tags: 'Sub-Bass, Gaita', rhythm: '6/8', arp: 0, prompt: 'Furruco Zuliano Sub C2', root: 36, norm: { attack: 0.015, decay: 0.35, sustain: 0.85, release: 0.38, cutoff: 0.42, resonance: 0.30, space_mix: 0.10, lfo_rate: 0.18, lfo_depth: 0.06 } },
  { name: 'Cyber-Criollo Neon Pluck', pack: 'pack-04', cat: 'Leads', tags: 'Synthwave, Plucks', rhythm: '6/8', arp: 3, prompt: 'Cyberpunk Pluck Lead Synth', root: 60, norm: { attack: 0.002, decay: 0.20, sustain: 0.68, release: 0.30, cutoff: 0.85, resonance: 0.28, space_mix: 0.35, lfo_rate: 0.48, lfo_depth: 0.18 } },
  { name: 'Capachos de Apure', pack: 'pack-01', cat: 'Percussion', tags: 'Percussion, Maracas', rhythm: '6/8', arp: 3, prompt: 'Maracas Llaneras Capachos C4', root: 60, norm: { attack: 0.001, decay: 0.10, sustain: 0.25, release: 0.12, cutoff: 0.96, resonance: 0.10, space_mix: 0.16, lfo_rate: 0.30, lfo_depth: 0.0 } },
  { name: 'Auyantepui Warm Analog', pack: 'pack-03', cat: 'Pads', tags: 'Pads, Warm Analog', rhythm: '4/4', arp: 0, prompt: 'Warm Analog Synth Pad', root: 60, norm: { attack: 0.32, decay: 0.45, sustain: 0.88, release: 0.58, cutoff: 0.72, resonance: 0.15, space_mix: 0.52, lfo_rate: 0.25, lfo_depth: 0.22 } },
  { name: 'Orinoco Solo Cello', pack: 'pack-03', cat: 'Strings', tags: 'Strings, Orchestral', rhythm: '4/4', arp: 0, prompt: 'Acoustic Solo Cello Expression', root: 48, norm: { attack: 0.18, decay: 0.35, sustain: 0.86, release: 0.45, cutoff: 0.78, resonance: 0.12, space_mix: 0.40, lfo_rate: 0.36, lfo_depth: 0.15 } },
  { name: 'Kukenan Crystal Bell', pack: 'pack-04', cat: 'Plucks', tags: 'Plucks, FM Bell', rhythm: '5/8', arp: 4, prompt: 'Metallic FM Bell Chime', root: 72, norm: { attack: 0.001, decay: 0.32, sustain: 0.50, release: 0.48, cutoff: 0.94, resonance: 0.16, space_mix: 0.45, lfo_rate: 0.30, lfo_depth: 0.10 } },
  { name: 'Catatumbo 808 Sub', pack: 'pack-05', cat: 'Basses', tags: 'Basses, 808 Sub', rhythm: '4/4', arp: 0, prompt: 'Deep 808 Sub Bass', root: 36, norm: { attack: 0.002, decay: 0.28, sustain: 0.75, release: 0.32, cutoff: 0.38, resonance: 0.25, space_mix: 0.08, lfo_rate: 0.20, lfo_depth: 0.0 } },
  { name: 'Canaima Brass Ensemble', pack: 'pack-04', cat: 'Leads', tags: 'Leads, Brass', rhythm: '4/4', arp: 1, prompt: 'Vintage Brass Ensemble Horns', root: 60, norm: { attack: 0.06, decay: 0.28, sustain: 0.82, release: 0.35, cutoff: 0.80, resonance: 0.18, space_mix: 0.32, lfo_rate: 0.34, lfo_depth: 0.12 } }
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

function applyPresetFromCatalog(preset) {
  if (!preset) return;
  activePresetId = preset.id;

  const promptInput = document.getElementById('prompt-input');
  if (promptInput) promptInput.value = preset.prompt;

  const browserLbl = document.getElementById('preset-browser-label');
  if (browserLbl) {
    browserLbl.innerText = `PRESET: ${preset.name.toUpperCase()} (1,250)`;
  }

  // Aplicar perfil DSP macro instantáneamente via IPC
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

  // Registrar evento ODD de carga de preset y sintetizar en el motor IA/Físico
  state.localUiPresetLoads = (state.localUiPresetLoads || 0) + 1;
  sendIpc(`preset:${preset.id}`);
  refreshFooterOddText();
  sendFastGenerate(preset.prompt);
  renderPresetTable();
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

  const stepPreset = (delta) => {
    const list = PRESET_CATALOG.filter((p) => {
      if (activePackFilter !== 'ALL' && p.pack !== activePackFilter) return false;
      if (activeCatFilter === 'FAV' && !favoritePresets.has(p.id)) return false;
      if (activeCatFilter !== 'ALL' && activeCatFilter !== 'FAV' && p.cat !== activeCatFilter) return false;
      return true;
    });
    if (list.length === 0) return;
    const curIdx = Math.max(0, list.findIndex((p) => p.id === activePresetId));
    const nextIdx = (curIdx + delta + list.length) % list.length;
    applyPresetFromCatalog(list[nextIdx]);
  };

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
    });
  });

  document.querySelectorAll('.pack-pill').forEach((pill) => {
    pill.addEventListener('click', () => {
      const packId = pill.dataset.pack || 'ALL';
      activePackFilter = packId;
      document.querySelectorAll('.pack-pill').forEach(b => b.classList.toggle('active', b === pill));
      setBrowserOpen(true);
    });
  });

  // Botones dedicados del módulo 6/8 Joropo Arp
  document.querySelectorAll('.arp-mode-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      const m = parseInt(btn.dataset.arp, 10) || 0;
      updateArpModeUI(m);
      sendIpc(`arp,${m}`);
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
  initKeyboard();
  initKeyboardEvents();
  initAiHub();
  initPresetBrowser();
  initWindowResizeControls();
  initAudioSettingsModal();
  fetchInitState();
  startTelemetryLoop();
  updateEngineBadge();

  // Redimensionar piano y oscilograma en resize
  window.addEventListener('resize', () => {
    initKeyboard();
  });

  sendIpc('log:initApp finished successfully!');
}

if (document.readyState === 'loading') {
  window.addEventListener('DOMContentLoaded', initApp);
  setTimeout(initApp, 60);
} else {
  initApp();
}


// -------------------------------------------------------------
// 10. REDIMENSIONADO NATIVO Y ESCALADO RESPONSIVO (STANDALONE & DAW)
// -------------------------------------------------------------
const BASE_UI_W = 980;
const BASE_UI_H = 720;
const UI_SCALE_STEPS = [1.0, 1.15, 1.3, 0.9];
let currentScaleIdx = 0;

function applyResponsiveViewport() {
  const container = document.querySelector('.plugin-container');
  const scaleBtn = document.getElementById('btn-ui-scale');
  if (!container) return;

  const vw = window.innerWidth || BASE_UI_W;
  const vh = window.innerHeight || BASE_UI_H;

  // Si la ventana es más pequeña que 920x660, aplicamos zoom proporcional automático para evitar recortes
  if (vw < 920 || vh < 660) {
    const z = Math.max(0.65, Math.min(vw / 920, vh / 660));
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
