(async function bootstrapDemo() {
  const state = {
    promptTokens: 0,
    completionTokens: 0,
  };

  async function loadConfig() {
    const response = await fetch('/__plugin_demo_config');
    return response.json();
  }

  function setOutput(id, value) {
    document.getElementById(id).textContent =
      typeof value === 'string' ? value : JSON.stringify(value, null, 2);
  }

  function updateUsage(promptTokens, completionTokens) {
    state.promptTokens = promptTokens;
    state.completionTokens = completionTokens;
    document.getElementById('prompt-tokens').textContent = String(promptTokens);
    document.getElementById('completion-tokens').textContent = String(completionTokens);
    document.getElementById('total-tokens').textContent = String(promptTokens + completionTokens);
  }

  const config = await loadConfig();
  document.getElementById('runner-url').textContent = 'URL: ' + config.runnerUrl;

  try {
    const healthResponse = await fetch(config.runnerUrl.replace(/\/$/, '') + '/health');
    document.getElementById('runner-status').textContent =
      healthResponse.ok ? 'Runner: reachable' : 'Runner: unavailable';
  } catch (error) {
    document.getElementById('runner-status').textContent = 'Runner: unreachable';
  }

  document.getElementById('validate-button').addEventListener('click', () => {
    const baseUrl = document.getElementById('base-url').value;
    const apiKey = document.getElementById('api-key').value;
    setOutput('validate-output', {
      ok: true,
      providerCode: config.providerCode,
      base_url: baseUrl,
      api_key_present: Boolean(apiKey),
      note: 'This is a scaffold response. Wire it to the real debug runtime later.',
    });
  });

  document.getElementById('list-models-button').addEventListener('click', () => {
    setOutput('models-output', [
      { code: config.providerCode + '_chat', label: 'Example Chat Model', mode: 'chat' },
      { code: config.providerCode + '_reasoning', label: 'Example Reasoning Model', mode: 'reasoning' },
      { code: config.providerCode + '_vision', label: 'Example Vision Model', mode: 'multimodal' },
    ]);
  });

  document.getElementById('stream-button').addEventListener('click', async () => {
    const prompt = document.getElementById('prompt-input').value.trim();
    const tokens = prompt ? Math.max(8, Math.ceil(prompt.length / 4)) : 8;
    const chunks = [
      'Streaming scaffold connected. ',
      'Replace this animation with real provider events. ',
      'Prompt preview: ' + prompt.slice(0, 48),
    ];

    setOutput('stream-output', '');
    let rendered = '';
    for (const chunk of chunks) {
      rendered += chunk;
      setOutput('stream-output', rendered);
      await new Promise((resolve) => setTimeout(resolve, 90));
    }

    updateUsage(tokens, 24);
  });

  document.getElementById('tool-button').addEventListener('click', () => {
    setOutput('tool-output', {
      tool_call: {
        tool: 'search_docs',
        arguments: { query: 'provider kernel contract' },
      },
      mcp_call: {
        server: 'docs',
        method: 'search',
      },
      status: 'scaffold_only',
    });
  });

  updateUsage(0, 0);
})();