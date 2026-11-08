//! Suite de certificación universal (`GATE-04`) sobre el motor de referencia `StereoDriveFilterEffect`.

#[allow(unused_imports)]
use base_plugin_core::contract_harness::{DspImpact, TestSignal};
use base_plugin_core::declare_plugin_contract;
use base_plugin_core::reference_engines::StereoDriveFilterEffect;

const REFERENCE_HTML: &str = r#"
<div id="plugin-root" data-skin="minimal">
  <div id="knob-drive" data-param="0"></div>
  <div id="knob-cutoff" data-param="1"></div>
  <div id="knob-mix" data-param="2"></div>
  <button id="btn-bypass" data-param="3"></button>
</div>
"#;

const REFERENCE_CSS: &str = r#"
[data-skin="minimal"] { --bg-primary: #f4f4f2; }
[data-skin="dark"] { --bg-primary: #121316; }
[data-skin="retro"] { --bg-primary: #d9d2c5; }
[data-skin="synth70"] { --bg-primary: #1b1917; }
[data-skin="gunmetal"] { --bg-primary: #1c2026; }
[data-skin="phosphor"] { --bg-primary: #07120b; }
"#;

declare_plugin_contract!(
    plugin_name: "ReferenceStereoDriveFilter",
    engine_type: StereoDriveFilterEffect,
    html_source: REFERENCE_HTML,
    css_source: REFERENCE_CSS,
    controls: [
        DriveKnob => {
            param_index: 0,
            dom_id: "knob-drive",
            ipc_cmd: "ParamSetValue",
            proof: DspImpact::AmplifiesRms,
            stimulus: TestSignal::LinearChirp
        },
        CutoffKnob => {
            param_index: 1,
            dom_id: "knob-cutoff",
            ipc_cmd: "ParamSetValue",
            proof: DspImpact::AltersFrequencyResponse,
            stimulus: TestSignal::LinearChirp
        },
        MixKnob => {
            param_index: 2,
            dom_id: "knob-mix",
            ipc_cmd: "ParamSetValue",
            proof: DspImpact::MutatesWaveform,
            stimulus: TestSignal::LinearChirp
        },
        BypassButton => {
            param_index: 3,
            dom_id: "btn-bypass",
            ipc_cmd: "ParamSetValue",
            proof: DspImpact::PassThroughEqual,
            stimulus: TestSignal::LinearChirp
        }
    ],
    skins: [
        "minimal",
        "dark",
        "retro",
        "synth70",
        "gunmetal",
        "phosphor"
    ]
);
