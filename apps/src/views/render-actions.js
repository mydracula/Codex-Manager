export function buildRenderActions({
  updateAccountSort,
  handleOpenUsageModal,
  setManualPreferredAccount,
  deleteAccount,
  refreshAccountsPage,
  toggleApiKeyStatus,
  deleteApiKey,
  updateApiKeyModel,
  copyApiKey,
}) {
  return {
    onUpdateSort: updateAccountSort,
    onOpenUsage: handleOpenUsageModal,
    onSetCurrentAccount: setManualPreferredAccount,
    onDeleteAccount: deleteAccount,
    onRefreshAccountPage: refreshAccountsPage,
    onToggleApiKeyStatus: toggleApiKeyStatus,
    onDeleteApiKey: deleteApiKey,
    onUpdateApiKeyModel: updateApiKeyModel,
    onCopyApiKey: copyApiKey,
  };
}
