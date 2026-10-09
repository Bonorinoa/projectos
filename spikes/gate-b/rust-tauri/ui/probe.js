const run = document.getElementById('run');
run.addEventListener('click', async () => {
  run.disabled = true;
  try { document.getElementById('result').textContent = JSON.stringify(await window.__TAURI__.core.invoke('graph_probe'), null, 2); }
  catch (error) { document.getElementById('result').textContent = String(error); }
  finally { run.disabled = false; }
});
