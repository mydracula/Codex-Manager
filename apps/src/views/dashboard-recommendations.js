import { dom } from "../ui/dom.js";
import { calcAvailability, remainingPercent } from "../utils/format.js";

export function renderRecommendations(accounts, usageMap, highlights = {}) {
  if (!dom.recommendations) return;
  dom.recommendations.innerHTML = "";
  const header = document.createElement("div");
  header.className = "panel-header";
  const title = document.createElement("h3");
  title.textContent = "最佳账号推荐";
  const hint = document.createElement("span");
  hint.className = "hint";
  hint.textContent = "按剩余额度";
  header.appendChild(title);
  header.appendChild(hint);
  dom.recommendations.appendChild(header);

  const primaryHighlight = highlights?.primaryRecommendation || null;
  const secondaryHighlight = highlights?.secondaryRecommendation || null;
  if (!accounts.length && !primaryHighlight && !secondaryHighlight) {
    const empty = document.createElement("div");
    empty.className = "hint";
    empty.textContent = "暂无可推荐账号";
    dom.recommendations.appendChild(empty);
    return;
  }

  const list = document.createElement("div");
  list.className = "mini-usage";

  const { primaryPick, secondaryPick } = pickBestRecommendations(accounts, usageMap);
  const primaryAccount = primaryHighlight || primaryPick?.account || null;
  const primaryRemain = primaryHighlight
    ? remainingPercent(primaryHighlight?.usage?.usedPercent)
    : primaryPick?.remain;
  const secondaryAccount = secondaryHighlight || secondaryPick?.account || null;
  const secondaryRemain = secondaryHighlight
    ? remainingPercent(secondaryHighlight?.usage?.secondaryUsedPercent)
    : secondaryPick?.remain;
  list.appendChild(renderRecommendationItem("用于 5小时", primaryAccount, primaryRemain));
  list.appendChild(renderRecommendationItem("用于 7天", secondaryAccount, secondaryRemain));

  dom.recommendations.appendChild(list);
}

export function pickBestRecommendations(accounts, usageMap) {
  let primaryPick = null;
  let secondaryPick = null;

  (accounts || []).forEach((account) => {
    const usage = usageMap.get(account.id);
    const status = calcAvailability(usage, account);
    // 中文注释：不可用账号不参与推荐（与网关候选池保持一致）。
    if (status.level === "warn" || status.level === "bad") {
      return;
    }
    const primaryRemain = remainingPercent(usage ? usage.usedPercent : null);
    const secondaryRemain = remainingPercent(
      usage ? usage.secondaryUsedPercent : null,
    );

    if (primaryRemain != null && (!primaryPick || primaryRemain > primaryPick.remain)) {
      primaryPick = { account, remain: primaryRemain };
    }
    if (secondaryRemain != null && (!secondaryPick || secondaryRemain > secondaryPick.remain)) {
      secondaryPick = { account, remain: secondaryRemain };
    }
  });

  return { primaryPick, secondaryPick };
}

function renderRecommendationItem(label, account, remain) {
  const item = document.createElement("div");
  item.className = "cell";
  const itemLabel = document.createElement("small");
  itemLabel.textContent = label;
  item.appendChild(itemLabel);
  if (!account) {
    const empty = document.createElement("strong");
    empty.textContent = "暂无账号";
    item.appendChild(empty);
    return item;
  }
  const accountLabel = document.createElement("strong");
  accountLabel.textContent = account.label || "-";
  const accountId = document.createElement("small");
  accountId.textContent = account.id || "-";
  item.appendChild(accountLabel);
  item.appendChild(accountId);
  const badge = document.createElement("span");
  badge.className = "status-tag status-ok";
  badge.textContent = remain == null ? "--" : `${remain}%`;
  item.appendChild(badge);
  return item;
}
