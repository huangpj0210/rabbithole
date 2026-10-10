/**
 * 单管理员控制台,无角色体系;登录即可管理。
 * @see https://umijs.org/docs/max/access#access
 */
export default function access(initialState: { currentUser?: API.CurrentUser } | undefined) {
  const { currentUser } = initialState ?? {};
  return {
    canAdmin: !!currentUser,
  };
}
