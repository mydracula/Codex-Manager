import test from "node:test";
import assert from "node:assert/strict";

import { state } from "../../state.js";
import {
  refreshAccounts,
  refreshAccountStats,
  refreshDashboardHighlights,
  refreshAccountsPage,
  refreshRequestLogs,
} from "../data.js";

function deferred() {
  let resolve = null;
  let reject = null;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  // 中文注释：避免某些取消路径下 deferred promise 未被 await 时触发 unhandledRejection。
  promise.catch(() => {});
  return { promise, resolve, reject };
}

test("refreshRequestLogs aborts stale request when query changes", async () => {
  const oldWindow = globalThis.window;
  const oldFetch = globalThis.fetch;
  const first = deferred();
  const second = deferred();
  const seenQueries = [];

  try {
    globalThis.window = {
      __TAURI__: {
        core: {
          invoke: async (method) => {
            if (method === "service_rpc_token") {
              return "test-token";
            }
            throw new Error(`unexpected invoke: ${method}`);
          },
        },
      },
    };
    globalThis.fetch = async (_url, options) => {
      const signal = options && options.signal;
      const query = JSON.parse(options.body).params.query;
      seenQueries.push(query);
      if (query === "old") {
        await first.promise;
        return {
          ok: true,
          json: async () => ({ result: { items: [{ id: "old" }] } }),
        };
      }
      await second.promise;
      return {
        ok: true,
        json: async () => ({ result: { items: [{ id: "new" }] } }),
      };
    };

    state.serviceAddr = "localhost:48760";
    state.requestLogList = [];

    const oldTask = refreshRequestLogs("old", { latestOnly: true });
    await Promise.resolve();
    const newTask = refreshRequestLogs("new", { latestOnly: true });

    first.reject(new DOMException("The operation was aborted.", "AbortError"));
    second.resolve();

    const oldApplied = await oldTask;
    const newApplied = await newTask;

    assert.equal(oldApplied, false);
    assert.equal(newApplied, true);
    assert.ok(seenQueries.includes("new"));
    assert.equal(state.requestLogList.length, 1);
    assert.equal(state.requestLogList[0].id, "new");
    assert.ok(state.requestLogList[0].__identity);
  } finally {
    globalThis.window = oldWindow;
    globalThis.fetch = oldFetch;
  }
});

test("refreshAccounts requests first page instead of full account list", async () => {
  const oldWindow = globalThis.window;
  const seenCalls = [];

  try {
    globalThis.window = {
      __TAURI__: {
        core: {
          invoke: async (method, params) => {
            seenCalls.push({ method, params });
            if (method === "service_account_list") {
              assert.equal(params.page, 1);
              assert.equal(params.pageSize, 5);
              return {
                result: {
                  items: [
                    { id: "acc-1", label: "账号1", groupName: "A组", sort: 1 },
                    { id: "acc-2", label: "账号2", groupName: "A组", sort: 2 },
                  ],
                  total: 200,
                  page: 1,
                  pageSize: 5,
                },
              };
            }
            if (method === "service_gateway_manual_account_get") {
              return { result: { accountId: "acc-2" } };
            }
            throw new Error(`unexpected invoke: ${method}`);
          },
        },
      },
    };

    state.accountList = [];
    state.accountPage = 1;
    state.accountPageSize = 5;
    state.accountSearch = "";
    state.accountFilter = "all";
    state.accountGroupFilter = "all";
    state.accountPageItems = [];
    state.accountPageTotal = 0;
    state.accountPageLoaded = false;
    state.manualPreferredAccountId = "";

    await refreshAccounts();

    assert.equal(seenCalls[0].method, "service_account_list");
    assert.equal(state.accountList.length, 2);
    assert.equal(state.accountPageItems.length, 2);
    assert.equal(state.accountPageTotal, 200);
    assert.equal(state.accountPageLoaded, true);
    assert.equal(state.manualPreferredAccountId, "acc-2");
  } finally {
    globalThis.window = oldWindow;
  }
});

test("refreshAccountStats normalizes backend counts", async () => {
  const oldWindow = globalThis.window;

  try {
    globalThis.window = {
      __TAURI__: {
        core: {
          invoke: async (method) => {
            if (method === "service_account_stats") {
              return {
                result: {
                  total: 200,
                  okCount: 150,
                  unavailableCount: 50,
                  lowCount: 18,
                },
              };
            }
            throw new Error(`unexpected invoke: ${method}`);
          },
        },
      },
    };

    state.accountStats = { total: 0, okCount: 0, unavailableCount: 0, lowCount: 0 };

    await refreshAccountStats();

    assert.deepEqual(state.accountStats, {
      total: 200,
      okCount: 150,
      unavailableCount: 50,
      lowCount: 18,
    });
  } finally {
    globalThis.window = oldWindow;
  }
});

test("refreshDashboardHighlights stores backend summary cards", async () => {
  const oldWindow = globalThis.window;

  try {
    globalThis.window = {
      __TAURI__: {
        core: {
          invoke: async (method) => {
            if (method === "service_account_dashboard_highlights") {
              return {
                result: {
                  current: {
                    id: "acc-current",
                    label: "当前账号",
                    statusLevel: "ok",
                    statusText: "可用",
                    usage: { usedPercent: 12, secondaryUsedPercent: 34 },
                  },
                  primaryRecommendation: {
                    id: "acc-p1",
                    label: "推荐1",
                    statusLevel: "ok",
                    statusText: "可用",
                    usage: { usedPercent: 8 },
                  },
                  secondaryRecommendation: {
                    id: "acc-p2",
                    label: "推荐2",
                    statusLevel: "ok",
                    statusText: "可用",
                    usage: { secondaryUsedPercent: 20 },
                  },
                },
              };
            }
            throw new Error(`unexpected invoke: ${method}`);
          },
        },
      },
    };

    state.dashboardHighlights = {
      current: null,
      primaryRecommendation: null,
      secondaryRecommendation: null,
    };

    await refreshDashboardHighlights();

    assert.equal(state.dashboardHighlights.current?.id, "acc-current");
    assert.equal(state.dashboardHighlights.primaryRecommendation?.id, "acc-p1");
    assert.equal(state.dashboardHighlights.secondaryRecommendation?.id, "acc-p2");
  } finally {
    globalThis.window = oldWindow;
  }
});

test("refreshAccountsPage falls back to local mode when backend does not return pagination fields", async () => {
  const oldWindow = globalThis.window;

  try {
    globalThis.window = {
      __TAURI__: {
        core: {
          invoke: async (method, params) => {
            if (method === "service_account_list") {
              assert.equal(params.page, 1);
              assert.equal(params.pageSize, 5);
              return {
                result: {
                  items: [
                    { id: "acc-1", label: "账号1", groupName: "A组", sort: 1 },
                    { id: "acc-2", label: "账号2", groupName: "A组", sort: 2 },
                  ],
                },
              };
            }
            throw new Error(`unexpected invoke: ${method}`);
          },
        },
      },
    };

    state.accountList = [];
    state.accountPage = 1;
    state.accountPageSize = 5;
    state.accountSearch = "";
    state.accountFilter = "all";
    state.accountGroupFilter = "all";
    state.accountPageItems = [];
    state.accountPageTotal = 0;
    state.accountPageLoaded = false;

    const applied = await refreshAccountsPage({ latestOnly: true });

    assert.equal(applied, true);
    assert.equal(state.accountPageLoaded, false);
    assert.equal(state.accountPageTotal, 0);
    assert.equal(state.accountPageItems.length, 0);
    assert.equal(state.accountList.length, 2);
    assert.equal(state.accountList[0].id, "acc-1");
  } finally {
    globalThis.window = oldWindow;
  }
});
