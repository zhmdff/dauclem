'use strict';
const { parentPort, workerData } = require('worker_threads');
const { indexAll } = require('./indexer');

let last = 0;
try {
  const n = indexAll(workerData.root, workerData.db, (done, total) => {
    const now = Date.now();
    if (now - last > 300 || done === total) {
      last = now;
      parentPort.postMessage({ type: 'progress', done, total });
    }
  });
  parentPort.postMessage({ type: 'done', changed: n });
} catch (e) {
  parentPort.postMessage({ type: 'error', message: String(e && e.stack || e) });
}
