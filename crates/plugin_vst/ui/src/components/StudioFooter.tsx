import React from "react";

export const StudioFooter: React.FC = () => {
  return (
    <footer className="studio-footer">
      <div className="footer-status">
        <span className="status-indicator"></span>
        <span id="footer-text">
          ENGINE: HYBRID-PHYSICAL • ROOT: C4 (261.6 Hz) • VEL: 95% • MOD ROUTES: 5 ACTIVE • 48.0 kHz / 32-BIT FLOAT
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
