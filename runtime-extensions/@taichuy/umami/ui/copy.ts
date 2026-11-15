export const copy = {
  en: {
    title: 'Umami connections', intro: 'Manage shared Umami connections. Your account is authenticated automatically when saving or using a connection.',
    create: 'Add connection', edit: 'Edit connection', editAction: 'Edit', name: 'Name', base_url: 'Umami URL', username: 'Username', password: 'Password',
    passwordHelp: 'Leave empty to keep the saved password.', passwordCreateHelp: 'Enter the password for this Umami account.',
    enabled: 'Enabled', disabled: 'Disabled', status: 'Status', has_password: 'Password', configured: 'Configured', missing: 'Not configured',
    actions: 'Actions', test: 'Test connection', testDraft: 'Test these settings', save: 'Save', cancel: 'Cancel', more: 'More actions',
    delete: 'Delete', deleteTitle: 'Delete connection?', deleteHelp: 'This removes the shared connection. Your Umami account and analytics data are kept.',
    search: 'Search connections', refresh: 'Refresh', columns: 'Columns', resetColumns: 'Reset columns', empty: 'No connections found', required: 'This field is required',
    created: 'Connection added', updated: 'Connection saved', deleted: 'Connection deleted', tested: 'Connection authenticated successfully', refreshed: 'Connections refreshed',
    forbidden: 'You do not have permission to perform this operation.', noList: 'You do not have permission to view Umami connections.',
    requestFailed: 'The operation could not be completed. Try again.', invalidInput: 'Check the connection settings and try again.', invalidUrl: 'Enter a valid HTTP or HTTPS Umami URL without credentials.',
    missingHost: 'This connection is no longer available. Refresh the list.', disabledHost: 'Enable the connection before testing or using it.', passwordRequired: 'Enter the Umami account password.',
    authenticationFailed: 'Umami could not authenticate this account. Check the username and password.', twoFactor: 'This account requires two-factor authentication, which is not supported for shared connections.',
    networkError: 'Umami could not be reached. Check the URL and try again.', umamiDenied: 'This Umami account does not have permission for this operation.',
    mcp: 'MCP settings', mcpHelp: 'Configure your AI client to use these shared connections in MCP settings.',
  },
  zh: {
    title: 'Umami 连接', intro: '管理共享 Umami 连接。保存或使用连接时，系统会自动验证账号。',
    create: '添加连接', edit: '编辑连接', editAction: '编辑', name: '名称', base_url: 'Umami 地址', username: '用户名', password: '密码',
    passwordHelp: '留空保留已保存的密码。', passwordCreateHelp: '请输入此 Umami 账号的密码。',
    enabled: '已启用', disabled: '已停用', status: '状态', has_password: '密码', configured: '已配置', missing: '未配置',
    actions: '操作', test: '测试连接', testDraft: '测试当前配置', save: '保存', cancel: '取消', more: '更多操作',
    delete: '删除', deleteTitle: '删除连接？', deleteHelp: '仅移除共享连接，Umami 账号及统计数据会保留。',
    search: '搜索连接', refresh: '刷新', columns: '显示列', resetColumns: '重置显示列', empty: '未找到连接', required: '请填写此项',
    created: '连接已添加', updated: '连接已保存', deleted: '连接已删除', tested: '连接验证成功', refreshed: '连接列表已刷新',
    forbidden: '你没有执行此操作的权限。', noList: '你没有查看 Umami 连接的权限。',
    requestFailed: '操作未完成，请重试。', invalidInput: '请检查连接配置后重试。', invalidUrl: '请输入有效的 HTTP 或 HTTPS Umami 地址，地址中不得包含账号密码。',
    missingHost: '此连接已不可用，请刷新列表。', disabledHost: '请先启用连接，再进行测试或使用。', passwordRequired: '请输入 Umami 账号密码。',
    authenticationFailed: 'Umami 账号验证失败，请检查用户名和密码。', twoFactor: '此账号需要双重认证，共享连接暂不支持此登录方式。',
    networkError: '无法连接 Umami，请检查地址后重试。', umamiDenied: '此 Umami 账号没有执行该操作的权限。',
    mcp: 'MCP 设置', mcpHelp: '在 MCP 设置中配置 AI 客户端，以使用这些共享连接。',
  },
};
export type Copy = typeof copy.en;

export function errorText(code: string, t: Copy) {
  const messages: Record<string, string> = {
    invalid_input: t.invalidInput, invalid_base_url: t.invalidUrl, host_not_found: t.missingHost,
    host_disabled: t.disabledHost, password_required: t.passwordRequired,
    umami_authentication_failed: t.authenticationFailed, umami_two_factor_unsupported: t.twoFactor,
    umami_network_error: t.networkError, umami_permission_denied: t.umamiDenied,
    forbidden: t.forbidden, permission_denied: t.forbidden, scope_mismatch: t.forbidden,
  };
  return messages[code] ?? t.requestFailed;
}
