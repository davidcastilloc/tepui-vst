import React from "react";

export const StudioFooter: React.FC = () => {
  return (
    <footer className="studio-footer">
      <div className="footer-status">
        <span className="status-indicator"></span>
        <span id="footer-text">
          ODD // BACKEND: HYBRID-PHYSICAL • SYNTH: 0µs • AI-PATCH: #1 • PITCH:
          261.6Hz • CACHE: 0H/0M • ROUTES: 5 • VEL: 95% • FAVS: 6 • PRESETS: 0
          • MATRIX: 0 • RESIZE: 0 • DSP BLK: 0
        </span>
      </div>
      <div className="footer-actions">
        <span className="shortcut-tip">
          <kbd>Shift</kbd> + Arrastre fino
        </span>
        <span className="shortcut-tip">
          <kbd>Doble Clic</kbd> reset
        </span>
      </div>
    </footer>
  );
};
