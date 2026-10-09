<script lang="ts" setup>
import { computed, reactive, ref } from 'vue';

import { Page } from '@vben/common-ui';

import {
  Button,
  Card,
  Form,
  FormItem,
  InputPassword,
  message,
} from 'ant-design-vue';

import { changePassword } from '#/api/rabbithole';
import { useAuthStore } from '#/store/auth';

const authStore = useAuthStore();
const submitting = ref(false);
const formState = reactive({
  oldPassword: '',
  newPassword: '',
  confirmPassword: '',
});

const mismatch = computed(
  () =>
    formState.confirmPassword.length > 0 &&
    formState.confirmPassword !== formState.newPassword,
);

async function submit() {
  if (formState.newPassword.length < 8) {
    message.warning('新密码至少 8 位');
    return;
  }
  if (mismatch.value) {
    message.warning('两次输入的新密码不一致');
    return;
  }
  submitting.value = true;
  try {
    await changePassword(formState.oldPassword, formState.newPassword);
    message.success('密码已修改,所有会话已失效,请重新登录');
    formState.oldPassword = '';
    formState.newPassword = '';
    formState.confirmPassword = '';
    // 后端已轮换会话密钥:当前令牌失效,回到登录页
    await authStore.logout();
  } catch (error: any) {
    message.error(error?.message ?? '修改失败');
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <Page>
    <Card title="修改密码" class="max-w-lg">
      <Form layout="vertical">
        <FormItem label="当前密码" required>
          <InputPassword
            v-model:value="formState.oldPassword"
            autocomplete="current-password"
            placeholder="输入当前密码"
          />
        </FormItem>
        <FormItem label="新密码(至少 8 位)" required>
          <InputPassword
            v-model:value="formState.newPassword"
            autocomplete="new-password"
            placeholder="输入新密码"
          />
        </FormItem>
        <FormItem
          label="确认新密码"
          required
          :validate-status="mismatch ? 'error' : undefined"
          :help="mismatch ? '两次输入的新密码不一致' : undefined"
        >
          <InputPassword
            v-model:value="formState.confirmPassword"
            autocomplete="new-password"
            placeholder="再次输入新密码"
          />
        </FormItem>
        <Button type="primary" :loading="submitting" @click="submit">
          修改密码
        </Button>
      </Form>
    </Card>
  </Page>
</template>
