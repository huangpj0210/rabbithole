import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from '@/App';
import './index.css';

const container = document.getElementById('root');
if (!container) {
  throw new Error('#root 容器缺失');
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

// 首屏占位移除(见 index.html 的 #rh-boot)
document.getElementById('rh-boot')?.remove();
