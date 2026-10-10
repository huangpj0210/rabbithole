import { changePassword } from '@/api/rabbithole';
import { useAuthStore } from '@/stores/auth';
import { useMutation } from '@tanstack/react-query';
import { history } from '@umijs/max';
import { PageContainer } from '@ant-design/pro-components';
import { App, Button, Card, Form, Input, Typography } from 'antd';
import React from 'react';

interface PasswordForm {
  oldPassword: string;
  newPassword: string;
  confirmPassword: string;
}

/** 设置页:修改密码(成功后强制重登)+ Prometheus 地址展示。 */
const SettingsPage: React.FC = () => {
  const [form] = Form.useForm<PasswordForm>();
  const { message } = App.useApp();

  const changeMut = useMutation({
    mutationFn: (v: PasswordForm) => changePassword(v.oldPassword, v.newPassword),
    onSuccess: () => {
      message.success('密码已修改,所有会话已失效,请重新登录');
      // 后端已轮换 JWT 密钥:当前令牌必然失效,清 token 回登录页
      useAuthStore.getState().clear();
      history.replace('/user/login');
    },
  });

  const metricsUrl = `${window.location.origin}/metrics`;

  return (
    <PageContainer subTitle="修改密码与部署信息" title="设置">
      <Card style={{ maxWidth: 560 }} title="修改密码">
        <Form
          form={form}
          labelCol={{ span: 6 }}
          layout="horizontal"
          onFinish={(values) => changeMut.mutate(values)}
          wrapperCol={{ span: 16 }}
        >
          <Form.Item label="当前密码" name="oldPassword" rules={[{ required: true, message: '请输入当前密码' }]}>
            <Input.Password autoComplete="current-password" placeholder="输入当前密码" />
          </Form.Item>
          <Form.Item
            label="新密码"
            name="newPassword"
            rules={[
              { required: true, message: '请输入新密码' },
              { min: 8, message: '新密码至少 8 位' },
            ]}
          >
            <Input.Password autoComplete="new-password" placeholder="至少 8 位" />
          </Form.Item>
          <Form.Item
            dependencies={['newPassword']}
            label="确认新密码"
            name="confirmPassword"
            rules={[
              { required: true, message: '请再次输入新密码' },
              ({ getFieldValue }) => ({
                validator(_, value) {
                  if (!value || getFieldValue('newPassword') === value) {
                    return Promise.resolve();
                  }
                  return Promise.reject(new Error('两次输入的新密码不一致'));
                },
              }),
            ]}
          >
            <Input.Password autoComplete="new-password" placeholder="再次输入新密码" />
          </Form.Item>
          <Form.Item style={{ marginBottom: 0 }} wrapperCol={{ offset: 6 }}>
            <Button htmlType="submit" loading={changeMut.isPending} type="primary">
              修改密码
            </Button>
          </Form.Item>
        </Form>
      </Card>

      <Card style={{ maxWidth: 560, marginTop: 16 }} title="监控指标">
        <Typography.Paragraph>
          Prometheus 抓取地址:
          <Typography.Text copyable>{metricsUrl}</Typography.Text>
        </Typography.Paragraph>
        <Typography.Paragraph type="secondary">
          需在服务端启用 metrics(enable_prometheus)后可用;抓取时携带管理台 Bearer 令牌。
        </Typography.Paragraph>
      </Card>
    </PageContainer>
  );
};

export default SettingsPage;
