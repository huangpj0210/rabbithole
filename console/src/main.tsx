import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import './index.css';

const container = document.getElementById('root');
if (!container) {
  throw new Error('#root 容器缺失');
}

createRoot(container).render(
  <StrictMode>
    <div className="p-6 text-sm text-muted-foreground">脚手架就位,待接入应用外壳。</div>
  </StrictMode>,
);

document.getElementById('rh-boot')?.remove();
