'use strict';

const DEFAULT_VALIDATE_MODEL = true;
const PARAMETER_FORM_SCHEMA_VERSION = '1.0.0';
const RESERVED_INVOCATION_PARAMETER_KEYS = new Set([
  'model',
  'messages',
  'stream',
  'response_format',
  'tools',
]);

function assertFetchAvailable() {
  if (typeof fetch !== 'function') {
    throw new Error('global fetch is not available in this Node runtime');
  }
}

function requireText(value, field) {
  const text = String(value || '').trim();
  if (!text) {
    throw new Error(`${field} is required`);
  }
  return text;
}

function normalizeProviderConfig(input) {
  const config = input && typeof input === 'object' ? input : {};
  return {
    base_url: requireText(config.base_url, 'base_url'),
    api_key: requireText(config.api_key, 'api_key'),
    organization: optionalText(config.organization),
    project: optionalText(config.project),
    api_version: optionalText(config.api_version),
    default_headers: parseDefaultHeaders(config.default_headers),
    validate_model:
      typeof config.validate_model === 'boolean'
        ? config.validate_model
        : DEFAULT_VALIDATE_MODEL,
  };
}

function optionalText(value) {
  const text = String(value || '').trim();
  return text || null;
}

function parseDefaultHeaders(value) {
  if (value == null || value === '') {
    return {};
  }
  if (typeof value === 'object' && !Array.isArray(value)) {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [key, String(entry)])
    );
  }
  if (typeof value !== 'string') {
    throw new Error('default_headers must be a JSON object string');
  }
  let parsed;
  try {
    parsed = JSON.parse(value);
  } catch (error) {
    throw new Error(`default_headers must be valid JSON: ${error.message}`);
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('default_headers must decode to a JSON object');
  }
  return Object.fromEntries(
    Object.entries(parsed).map(([key, entry]) => [key, String(entry)])
  );
}

function buildHeaders(config, includeJsonBody) {
  const headers = {
    Accept: 'application/json',
    ...config.default_headers,
  };
  if (includeJsonBody) {
    headers['Content-Type'] = 'application/json';
  }
  headers.Authorization = `Bearer ${config.api_key}`;
  if (config.organization) {
    headers['OpenAI-Organization'] = config.organization;
  }
  if (config.project) {
    headers['OpenAI-Project'] = config.project;
  }
  return headers;
}

function buildUrl(config, pathname) {
  const baseUrl = config.base_url.replace(/\/+$/, '');
  const url = new URL(baseUrl + pathname);
  if (config.api_version) {
    url.searchParams.set('api-version', config.api_version);
  }
  return url.toString();
}

async function requestJson(config, pathname, options = {}) {
  assertFetchAvailable();
  const response = await fetch(buildUrl(config, pathname), {
    method: options.method || 'GET',
    headers: buildHeaders(config, Boolean(options.body)),
    body: options.body ? JSON.stringify(options.body) : undefined,
  });
  const payload = await readJsonResponse(response);
  if (!response.ok) {
    const errorMessage = payload.error?.message || payload.message || JSON.stringify(payload);
    throw new Error(`${response.status} ${response.statusText}: ${errorMessage}`);
  }
  return payload;
}

async function readJsonResponse(response) {
  const text = await response.text();
  if (!text.trim()) {
    return {};
  }
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new Error(`provider returned invalid JSON: ${error.message}`);
  }
}

function normalizeModelEntry(entry) {
  const modelId = requireText(entry.id || entry.model_id, 'model_id');
  return {
    model_id: modelId,
    display_name: modelId,
    source: 'dynamic',
    supports_streaming: true,
    supports_tool_call: true,
    supports_multimodal: false,
    context_window: null,
    max_output_tokens: null,
    parameter_form: buildDefaultParameterForm(),
    provider_metadata: {
      owned_by: entry.owned_by || null,
      created: entry.created || null,
    },
  };
}

function buildDefaultParameterForm() {
  return {
    schema_version: PARAMETER_FORM_SCHEMA_VERSION,
    title: 'LLM Parameters',
    description: 'Common OpenAI-compatible chat completion parameters.',
    fields: [
      {
        key: 'temperature',
        label: 'Temperature',
        type: 'number',
        control: 'slider',
        group: 'sampling',
        order: 10,
        advanced: false,
        required: false,
        send_mode: 'optional',
        enabled_by_default: true,
        description: 'Controls randomness. Lower values are more deterministic.',
        default_value: 0.7,
        min: 0,
        max: 2,
        step: 0.1,
        precision: 1,
      },
      {
        key: 'top_p',
        label: 'Top P',
        type: 'number',
        control: 'slider',
        group: 'sampling',
        order: 20,
        advanced: true,
        required: false,
        send_mode: 'optional',
        enabled_by_default: false,
        description: 'Uses nucleus sampling to limit token candidates.',
        default_value: 1,
        min: 0,
        max: 1,
        step: 0.05,
        precision: 2,
      },
      {
        key: 'max_tokens',
        label: 'Max Tokens',
        type: 'integer',
        control: 'number',
        group: 'limits',
        order: 30,
        advanced: false,
        required: false,
        send_mode: 'optional',
        enabled_by_default: false,
        description: 'Caps the number of tokens generated in the response.',
        min: 1,
        step: 1,
      },
      {
        key: 'seed',
        label: 'Seed',
        type: 'integer',
        control: 'number',
        group: 'sampling',
        order: 40,
        advanced: true,
        required: false,
        send_mode: 'optional',
        enabled_by_default: false,
        description: 'Best-effort seed for repeatable outputs when supported.',
        min: 0,
        step: 1,
      },
    ],
  };
}

function buildInvocationMessages(request) {
  const messages = [];
  if (request.system) {
    messages.push({ role: 'system', content: request.system });
  }
  for (const message of Array.isArray(request.messages) ? request.messages : []) {
    messages.push({
      role: message.role,
      content: typeof message.content === 'string' ? message.content : JSON.stringify(message.content),
    });
  }
  return messages;
}

function pickDefinedFields(input) {
  return Object.fromEntries(
    Object.entries(input).filter(([, value]) => value !== undefined && value !== null)
  );
}

function normalizeModelParameters(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return {};
  }
  return Object.fromEntries(
    Object.entries(value).filter(
      ([key, entry]) =>
        !RESERVED_INVOCATION_PARAMETER_KEYS.has(key) && entry !== undefined && entry !== null
    )
  );
}

function buildLegacyModelParameters(request) {
  return pickDefinedFields({
    temperature: request.temperature,
    top_p: request.top_p,
    presence_penalty: request.presence_penalty,
    frequency_penalty: request.frequency_penalty,
    max_tokens: request.max_tokens,
    seed: request.seed,
  });
}

function extractContent(message) {
  if (!message) {
    return '';
  }
  if (typeof message.content === 'string') {
    return message.content;
  }
  if (!Array.isArray(message.content)) {
    return '';
  }
  return message.content
    .filter((part) => part && part.type === 'text')
    .map((part) => part.text || '')
    .join('');
}

function parseToolArguments(rawArguments) {
  if (!rawArguments) {
    return {};
  }
  if (typeof rawArguments !== 'string') {
    return rawArguments;
  }
  try {
    return JSON.parse(rawArguments);
  } catch (_error) {
    return { raw: rawArguments };
  }
}

function normalizeToolCalls(toolCalls) {
  if (!Array.isArray(toolCalls)) {
    return [];
  }
  return toolCalls.map((toolCall, index) => ({
    id: toolCall.id || `tool_call_${index + 1}`,
    name: toolCall.function?.name || 'unknown_tool',
    arguments: parseToolArguments(toolCall.function?.arguments),
  }));
}

function normalizeUsage(usage) {
  return {
    input_tokens: numberOrNull(usage?.prompt_tokens),
    output_tokens: numberOrNull(usage?.completion_tokens),
    reasoning_tokens: numberOrNull(usage?.reasoning_tokens),
    cache_read_tokens: numberOrNull(usage?.prompt_tokens_details?.cached_tokens),
    cache_write_tokens: numberOrNull(usage?.completion_tokens_details?.cached_tokens),
    total_tokens: numberOrNull(usage?.total_tokens),
  };
}

function numberOrNull(value) {
  return Number.isFinite(value) ? value : null;
}

function normalizeFinishReason(finishReason, toolCalls) {
  if (toolCalls.length > 0 || finishReason === 'tool_calls') {
    return 'tool_call';
  }
  switch (finishReason) {
    case 'stop':
      return 'stop';
    case 'length':
      return 'length';
    case 'content_filter':
      return 'content_filter';
    default:
      return 'unknown';
  }
}

module.exports = {
  providerCode: 'openai_compatible',

  async validateProviderCredentials(input) {
    const config = normalizeProviderConfig(input);
    const payload = await requestJson(config, '/models');
    return {
      ok: true,
      provider_code: 'openai_compatible',
      sanitized: {
        base_url: config.base_url,
        api_key: '***',
        organization: config.organization,
        project: config.project,
        api_version: config.api_version,
        default_headers: Object.keys(config.default_headers),
      },
      model_count: Array.isArray(payload.data) ? payload.data.length : 0,
    };
  },

  async listModels(input) {
    const config = normalizeProviderConfig(input);
    const payload = await requestJson(config, '/models');
    return (payload.data || []).map(normalizeModelEntry);
  },

  async invoke(request) {
    const config = normalizeProviderConfig(request.provider_config);
    const messages = buildInvocationMessages(request);
    const body = {
      model: requireText(request.model, 'model'),
      messages,
      stream: false,
      response_format: request.response_format,
      tools:
        Array.isArray(request.tools) && request.tools.length > 0
          ? request.tools
          : undefined,
      ...buildLegacyModelParameters(request),
      ...normalizeModelParameters(request.model_parameters),
    };
    const payload = await requestJson(config, '/chat/completions', {
      method: 'POST',
      body: pickDefinedFields(body),
    });
    const choice = Array.isArray(payload.choices) ? payload.choices[0] || {} : {};
    const message = choice.message || {};
    const text = extractContent(message);
    const toolCalls = normalizeToolCalls(message.tool_calls);
    const usage = normalizeUsage(payload.usage || {});
    const finishReason = normalizeFinishReason(choice.finish_reason, toolCalls);
    const events = [];

    if (text) {
      events.push({
        type: 'text_delta',
        delta: text,
      });
    }
    for (const call of toolCalls) {
      events.push({
        type: 'tool_call_commit',
        call,
      });
    }
    if (usage.total_tokens != null || usage.input_tokens != null || usage.output_tokens != null) {
      events.push({
        type: 'usage_snapshot',
        usage,
      });
    }
    events.push({
      type: 'finish',
      reason: finishReason,
    });

    return {
      events,
      result: {
        final_content: text || null,
        tool_calls: toolCalls,
        mcp_calls: [],
        usage,
        finish_reason: finishReason,
        provider_metadata: {
          request_model: request.model,
          response_model: payload.model || request.model,
          response_id: payload.id || null,
          created: payload.created || null,
          system_fingerprint: payload.system_fingerprint || null,
        },
      },
    };
  },
};
