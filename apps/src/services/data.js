import { state } from "../state.js";
import * as api from "../api.js";

let requestLogRefreshSeq = 0;
let requestLogInFlight = null;
let accountPageRefreshSeq = 0;
const DEFAULT_REQUEST_LOG_TODAY_SUMMARY = {
  todayTokens: 0,
  cachedInputTokens: 0,
  reasoningOutputTokens: 0,
  estimatedCost: 0,
};

function ensureRpcSuccess(result, fallbackMessage) {
  if (result && typeof result === "object" && typeof result.error === "string" && result.error) {
    throw new Error(result.error);
  }
  if (result == null) {
    throw new Error(fallbackMessage);
  }
  return result;
}

function isCommandMissingError(err) {
  const msg = String(err && err.message ? err.message : err).toLowerCase();
  if (
    msg.includes("not found")
    || msg.includes("unknown command")
    || msg.includes("no such command")
    || msg.includes("not managed")
    || msg.includes("does not exist")
  ) {
    return true;
  }
  return msg.includes("invalid args") && msg.includes("for command");
}

function readPath(source, path) {
  const steps = String(path).split(".");
  let cursor = source;
  for (const step of steps) {
    if (!cursor || typeof cursor !== "object" || !(step in cursor)) {
      return undefined;
    }
    cursor = cursor[step];
  }
  return cursor;
}

function toFiniteNumber(value) {
  if (typeof value === "number") {
    return Number.isFinite(value) ? value : null;
  }
  if (typeof value === "string") {
    const normalized = value.trim();
    if (!normalized) return null;
    const parsed = Number(normalized);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

function pickNumber(source, paths, fallback = 0) {
  for (const path of paths) {
    const parsed = toFiniteNumber(readPath(source, path));
    if (parsed != null) {
      return parsed;
    }
  }
  return fallback;
}

function isAbortError(err) {
  return Boolean(err && typeof err === "object" && err.name === "AbortError");
}

function buildRequestLogIdentity(item, index) {
  if (item && typeof item === "object" && item.id != null && String(item.id).trim()) {
    return String(item.id);
  }
  return [
    item?.createdAt ?? "",
    item?.method ?? "",
    item?.statusCode ?? "",
    item?.accountId ?? "",
    item?.keyId ?? "",
    index,
  ].join("|");
}

// 刷新账号列表（仅拉首屏分页，避免启动时全量加载大量账号）
export async function refreshAccounts() {
  const res = ensureRpcSuccess(
    await api.serviceAccountList({
      page: 1,
      pageSize: state.accountPageSize,
      query: state.accountSearch,
      filter: state.accountFilter,
      groupFilter: state.accountGroupFilter,
    }),
    "读取账号列表失败",
  );
  state.accountList = Array.isArray(res.items) ? res.items : [];
  const total = Number(res.total);
  const page = Number(res.page);
  const pageSize = Number(res.pageSize);
  if (Number.isFinite(total) && Number.isFinite(page) && Number.isFinite(pageSize)) {
    state.accountPageItems = state.accountList;
    state.accountPageTotal = Math.max(0, total);
    state.accountPage = page > 0 ? Math.trunc(page) : 1;
    state.accountPageSize = pageSize > 0 ? Math.trunc(pageSize) : state.accountPageSize;
    state.accountPageLoaded = true;
  }
  try {
    const manual = await api.serviceGatewayManualAccountGet();
    state.manualPreferredAccountId = String(manual?.accountId || "").trim();
  } catch {
    state.manualPreferredAccountId = "";
  }
}

// 刷新账号页分页数据
export async function refreshAccountsPage(options = {}) {
  const latestOnly = options.latestOnly !== false;
  const seq = ++accountPageRefreshSeq;
  const res = ensureRpcSuccess(
    await api.serviceAccountList({
      page: state.accountPage,
      pageSize: state.accountPageSize,
      query: state.accountSearch,
      filter: state.accountFilter,
      groupFilter: state.accountGroupFilter,
    }),
    "读取账号分页失败",
  );
  if (latestOnly && seq !== accountPageRefreshSeq) {
    return false;
  }

  const nextItems = Array.isArray(res.items) ? res.items : [];
  const nextTotal = Number(res.total);
  const nextPage = Number(res.page);
  const nextPageSize = Number(res.pageSize);
  const hasRemotePagination =
    Number.isFinite(nextTotal)
    && Number.isFinite(nextPage)
    && Number.isFinite(nextPageSize);

  // 中文注释：兼容旧版 service。
  // 旧版 account/list 只有 items，没有 total/page/pageSize；
  // 如果这里硬切到远端分页模式，账号页会被误判成“0 条数据”。
  if (!hasRemotePagination) {
    state.accountPageItems = [];
    state.accountPageTotal = 0;
    state.accountPageLoaded = false;
    if (nextItems.length > 0) {
      state.accountList = nextItems;
    }
    return true;
  }

  state.accountPageItems = nextItems;
  state.accountPageTotal = Math.max(0, nextTotal);
  state.accountPage = nextPage > 0 ? Math.trunc(nextPage) : 1;
  state.accountPageSize = nextPageSize > 0 ? Math.trunc(nextPageSize) : state.accountPageSize;
  state.accountPageLoaded = true;
  return true;
}

export async function refreshAccountStats() {
  const res = ensureRpcSuccess(await api.serviceAccountStats(), "读取账号统计失败");
  state.accountStats = {
    total: Number.isFinite(Number(res.total)) ? Math.max(0, Math.trunc(Number(res.total))) : 0,
    okCount: Number.isFinite(Number(res.okCount)) ? Math.max(0, Math.trunc(Number(res.okCount))) : 0,
    unavailableCount: Number.isFinite(Number(res.unavailableCount))
      ? Math.max(0, Math.trunc(Number(res.unavailableCount)))
      : 0,
    lowCount: Number.isFinite(Number(res.lowCount)) ? Math.max(0, Math.trunc(Number(res.lowCount))) : 0,
  };
}

export async function refreshDashboardHighlights() {
  const res = ensureRpcSuccess(
    await api.serviceAccountDashboardHighlights(),
    "读取仪表盘账号摘要失败",
  );
  state.dashboardHighlights = {
    current: res?.current || null,
    primaryRecommendation: res?.primaryRecommendation || null,
    secondaryRecommendation: res?.secondaryRecommendation || null,
  };
}

// 刷新用量列表
export async function refreshUsageList(options = {}) {
  const refreshRemote = options && options.refreshRemote === true;
  if (refreshRemote) {
    await ensureRpcSuccess(await api.serviceUsageRefresh(), "刷新用量失败");
  }
  const res = ensureRpcSuccess(await api.serviceUsageList(), "读取用量列表失败");
  state.usageList = Array.isArray(res.items) ? res.items : [];
}

// 刷新 API Key 列表
export async function refreshApiKeys() {
  const res = ensureRpcSuccess(await api.serviceApiKeyList(), "读取平台密钥列表失败");
  state.apiKeyList = Array.isArray(res.items) ? res.items : [];
}

// 刷新模型下拉选项（来自平台上游 /v1/models）
export async function refreshApiModels(options = {}) {
  const refreshRemote = options && options.refreshRemote === true;
  const res = ensureRpcSuccess(
    await api.serviceApiKeyModels({ refreshRemote }),
    "读取模型列表失败",
  );
  state.apiModelOptions = Array.isArray(res.items) ? res.items : [];
}

// 刷新请求日志（按关键字过滤）
export async function refreshRequestLogs(query, options = {}) {
  const latestOnly = options.latestOnly !== false;
  const normalizedQuery = query || null;
  const requestKey = `${normalizedQuery ?? ""}::300`;
  const seq = ++requestLogRefreshSeq;

  if (requestLogInFlight && requestLogInFlight.key !== requestKey) {
    requestLogInFlight.controller.abort();
    requestLogInFlight = null;
  }

  if (!requestLogInFlight || requestLogInFlight.key !== requestKey) {
    const controller = new AbortController();
    const timeoutMs = Number.isFinite(options.timeoutMs) ? Math.max(0, options.timeoutMs) : 15000;
    requestLogInFlight = {
      key: requestKey,
      controller,
      promise: (async () => ensureRpcSuccess(
        await api.serviceRequestLogList(normalizedQuery, 300, {
          signal: controller.signal,
          timeoutMs,
        }),
        "读取请求日志失败",
      ))(),
    };
  }

  const inFlight = requestLogInFlight;
  let res = null;
  try {
    res = await inFlight.promise;
  } catch (err) {
    if (isAbortError(err)) {
      return false;
    }
    throw err;
  } finally {
    if (requestLogInFlight === inFlight) {
      requestLogInFlight = null;
    }
  }
  if (latestOnly && seq !== requestLogRefreshSeq) {
    return false;
  }
  const items = Array.isArray(res.items) ? res.items : [];
  // 中文注释：预计算 identity，避免 render 阶段频繁 join 字符串造成 GC 抖动。
  for (let i = 0; i < items.length; i += 1) {
    const item = items[i];
    if (item && typeof item === "object" && !item.__identity) {
      item.__identity = buildRequestLogIdentity(item, i);
    }
  }
  state.requestLogList = items;
  return true;
}

export async function clearRequestLogs() {
  return ensureRpcSuccess(await api.serviceRequestLogClear(), "清空请求日志失败");
}

export async function refreshRequestLogTodaySummary() {
  try {
    const res = ensureRpcSuccess(
      await api.serviceRequestLogTodaySummary(),
      "读取今日请求汇总失败",
    );
    const inputTokens = pickNumber(res, [
      "inputTokens",
      "promptTokens",
      "tokens.input",
      "result.inputTokens",
      "result.promptTokens",
      "result.tokens.input",
    ], 0);
    const outputTokens = pickNumber(res, [
      "outputTokens",
      "completionTokens",
      "tokens.output",
      "result.outputTokens",
      "result.completionTokens",
      "result.tokens.output",
    ], 0);
    const cachedInputTokens = pickNumber(res, [
      "cachedInputTokens",
      "cachedTokens",
      "tokens.cachedInput",
      "usage.cachedInputTokens",
      "usage.cachedTokens",
      "result.cachedInputTokens",
      "result.cachedTokens",
      "result.tokens.cachedInput",
      "result.usage.cachedInputTokens",
      "result.usage.cachedTokens",
    ], 0);
    const reasoningOutputTokens = pickNumber(res, [
      "reasoningOutputTokens",
      "reasoningTokens",
      "tokens.reasoningOutput",
      "usage.reasoningOutputTokens",
      "usage.reasoningTokens",
      "result.reasoningOutputTokens",
      "result.reasoningTokens",
      "result.tokens.reasoningOutput",
      "result.usage.reasoningOutputTokens",
      "result.usage.reasoningTokens",
    ], 0);
    const todayTokens = pickNumber(res, [
      "todayTokens",
      "totalTokens",
      "tokenTotal",
      "tokens.total",
      "result.todayTokens",
      "result.totalTokens",
      "result.tokenTotal",
      "result.tokens.total",
    ], Math.max(0, inputTokens - cachedInputTokens) + outputTokens);
    const estimatedCost = pickNumber(res, [
      "estimatedCost",
      "cost",
      "costEstimate",
      "todayCost",
      "result.estimatedCost",
      "result.cost",
      "result.costEstimate",
      "result.todayCost",
    ], 0);
    state.requestLogTodaySummary = {
      todayTokens: Math.max(0, todayTokens),
      cachedInputTokens: Math.max(0, cachedInputTokens),
      reasoningOutputTokens: Math.max(0, reasoningOutputTokens),
      estimatedCost: Math.max(0, estimatedCost),
    };
  } catch (err) {
    if (!isCommandMissingError(err)) {
      throw err;
    }
    state.requestLogTodaySummary = { ...DEFAULT_REQUEST_LOG_TODAY_SUMMARY };
  }
}
