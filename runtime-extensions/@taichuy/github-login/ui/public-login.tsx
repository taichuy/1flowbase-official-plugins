import { GithubOutlined, ArrowLeftOutlined } from '@ant-design/icons';
import { useState } from 'react';
import { Alert, Button, Space } from 'antd';

type AuthContext = {
  inputs: { authenticator_selection_available?: boolean };
  auth: { redirectToExternalLogin(): void | Promise<void> };
  events: { emit(event: string): void };
};

export default function GitHubLogin({ ctx }: { ctx: AuthContext }) {
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState(false);
  const signIn = async () => {
    setPending(true);
    setFailed(false);
    try {
      await ctx.auth.redirectToExternalLogin();
    } catch {
      setFailed(true);
    } finally {
      setPending(false);
    }
  };
  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      {ctx.inputs.authenticator_selection_available ? (
        <Button aria-label="返回其他登录方式" type="text" disabled={pending}
          icon={<ArrowLeftOutlined aria-hidden="true" />}
          onClick={() => ctx.events.emit('authenticator_selector_requested')} />
      ) : null}
      {failed ? <Alert type="error" showIcon message="无法开始 GitHub 登录，请重试。" /> : null}
      <Button block size="large" type="primary" icon={<GithubOutlined aria-hidden="true" />}
        loading={pending} onClick={signIn}>使用 GitHub 登录</Button>
    </Space>
  );
}
