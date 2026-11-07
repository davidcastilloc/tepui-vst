import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { initApp } from './engine/oledEngine';

function mountTepuiReactApp() {
  const shellEl = document.getElementById('plugin-shell');
  if (shellEl && !(shellEl as any).__tepuiReactMounted) {
    (shellEl as any).__tepuiReactMounted = true;
    const root = createRoot(shellEl);
    root.render(<App />);
  } else {
    initApp();
  }
}

if (document.readyState === 'loading') {
  window.addEventListener('DOMContentLoaded', mountTepuiReactApp);
  setTimeout(mountTepuiReactApp, 60);
} else {
  mountTepuiReactApp();
}
