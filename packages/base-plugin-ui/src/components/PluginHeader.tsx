import React from 'react';
import { PluginLevelMeter } from './PluginLevelMeter';

export type HardwareSkinId =
  | 'minimal'
  | 'dark'
  | 'retro'
  | 'synth70'
  | 'gunmetal'
  | 'phosphor';

export interface PluginHeaderProps {
  pluginTitle: string;
  vendorName: string;
  activeSkin: HardwareSkinId;
  onSelectSkin: (skin: HardwareSkinId) => void;
  bypassed?: boolean;
  onToggleBypass?: () => void;
}

const HARDWARE_SKINS: { id: HardwareSkinId; label: string }[] = [
  { id: 'minimal', label: 'MINIMAL' },
  { id: 'dark', label: 'DARK' },
  { id: 'retro', label: 'RETRO' },
  { id: 'synth70', label: 'SYNTH 70' },
  { id: 'gunmetal', label: 'GUNMETAL' },
  { id: 'phosphor', label: 'PHOSPHOR' },
];

/**
 * Barra superior de estudio agnóstica con selector de 6 skins de hardware, bypass y medidor estéreo a 60 FPS.
 */
export const PluginHeader: React.FC<PluginHeaderProps> = ({
  pluginTitle,
  vendorName,
  activeSkin,
  onSelectSkin,
  bypassed = false,
  onToggleBypass,
}) => {
  return (
    <header className="plugin-shell-header">
      <div className="plugin-shell-brand">
        <span className="plugin-shell-title">{pluginTitle}</span>
        <span className="plugin-shell-vendor">{vendorName}</span>
      </div>
      <div className="plugin-shell-skins" role="group" aria-label="Hardware Skins">
        {HARDWARE_SKINS.map((s) => (
          <button
            key={s.id}
            type="button"
            className={`plugin-skin-btn ${activeSkin === s.id ? 'is-active' : ''}`}
            onClick={() => onSelectSkin(s.id)}
          >
            {s.label}
          </button>
        ))}
      </div>
      <div className="plugin-shell-actions">
        {onToggleBypass && (
          <button
            id="btn-bypass"
            type="button"
            className={`plugin-bypass-btn ${bypassed ? 'is-bypassed' : ''}`}
            onClick={onToggleBypass}
          >
            {bypassed ? 'BYPASSED' : 'ACTIVE'}
          </button>
        )}
        <PluginLevelMeter orientation="horizontal" />
      </div>
    </header>
  );
};
