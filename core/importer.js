'use strict';
// 导入流水线：文件内容 → 预览 → 入库（纯逻辑，不碰窗口）
// 支持 UTF-8 / GBK / GB18030 / UTF-16（含 BOM）

const { alignText } = require('./align');
const { splitEN, splitCN, smartGroups } = require('./split');

// Buffer → 文本（UTF-8），并做编码体检
function readText(buf) {
  const b = Buffer.from(buf);
  if (!b.length) throw new Error('文件是空的。');
  let text;
  // BOM 优先，避免把 UTF-16 误当成含大量 NUL 的 UTF-8。
  if (b[0] === 0xFF && b[1] === 0xFE) {
    text = new TextDecoder('utf-16le').decode(b.subarray(2));
  } else if (b[0] === 0xFE && b[1] === 0xFF) {
    text = new TextDecoder('utf-16be').decode(b.subarray(2));
  } else if (b[0] === 0xEF && b[1] === 0xBB && b[2] === 0xBF) {
    text = new TextDecoder('utf-8', { fatal: true }).decode(b.subarray(3));
  } else {
    // 严格 UTF-8 成功则直接采用；失败时按中文 Windows 文本尝试 GB18030。
    try {
      text = new TextDecoder('utf-8', { fatal: true }).decode(b);
    } catch (_) {
      try { text = new TextDecoder('gb18030', { fatal: false }).decode(b); }
      catch (e) { throw new Error('无法识别文件编码，请另存为 UTF-8 后再导入。'); }
    }
  }
  text = text.replace(/^\uFEFF/, '');
  if (text.includes('\u0000')) throw new Error('文件包含无效的二进制内容，无法作为文本导入。');
  const bad = (text.match(/\uFFFD/g) || []).length;
  if (bad > 10 && bad / Math.max(1, text.length) > 0.002)
    throw new Error('文件里出现大量乱码，请检查文件编码后再导入。');
  if (!text.trim()) throw new Error('文件是空的。');
  return text;
}

// 段内英文句与中文句配对：句数相等才逐句对，否则整段中文挂到每张卡
function zipPair(enArr, cnArr) {
  if (!cnArr.length) return enArr.map(en => ({ en, cn: '' }));
  if (cnArr.length === enArr.length) return enArr.map((en, i) => ({ en, cn: cnArr[i] }));
  return enArr.map(en => ({ en, cn: cnArr.join('') }));
}

// align 结果 → 练习卡
function buildCards(sections, unit) {
  const cards = [];
  const warnings = [];
  let cnOnly = 0;
  for (const sec of sections) {
    for (const pair of sec.pairs) {
      if (!pair.en) { if (pair.cn) cnOnly++; continue; }
      let enParts, cnParts;
      if (unit === 'paragraph') {
        enParts = [pair.en];
        cnParts = pair.cn ? [pair.cn] : [];
      } else {
        enParts = splitEN(pair.en);
        cnParts = pair.cn ? splitCN(pair.cn) : [];
        if (unit === 'smart') {
          const groups = smartGroups(enParts, 30);
          // 中英句数一致才能按组合并译文；否则保持原样，交给 zipPair 兜底
          const cnMerged = cnParts.length === enParts.length
            ? groups.map(g => g.map(i => cnParts[i]).join(''))
            : null;
          enParts = groups.map(g => g.map(i => enParts[i]).join(' '));
          if (cnMerged) cnParts = cnMerged;
        }
      }
      for (const z of zipPair(enParts, cnParts)) {
        cards.push({ en: z.en, cn: z.cn });
      }
    }
  }
  if (cnOnly) warnings.push(`有 ${cnOnly} 个中文段落没有对应的英文，已跳过`);
  return { cards, warnings };
}

// 完整走一遍解析（预览和入库共用）
function parse({ text, rule = 'auto', unit = 'sentence' }) {
  const al = alignText(text, rule);
  const built = buildCards(al.sections, unit);
  const pairs = al.sections.reduce((n, s) => n + s.pairs.length, 0);
  const enParas = al.sections.reduce((n, s) => n + s.pairs.filter(p => p.en).length, 0);
  const cnParas = al.sections.reduce((n, s) => n + s.pairs.filter(p => p.cn).length, 0);
  return {
    ruleCount: al.ruleCount,
    stats: { enParas, cnParas, pairs, cards: built.cards.length },
    warnings: [...al.warnings, ...built.warnings],
    cards: built.cards
  };
}

// 预览：只返回统计与样例，不返回全量
function preview(params) {
  const r = parse(params);
  return {
    ruleCount: r.ruleCount,
    stats: r.stats,
    warnings: r.warnings,
    sample: r.cards.slice(0, 12)
  };
}

// 组装成书（调用方拿到后写库）
function buildBook({ text, rule = 'auto', unit = 'sentence', title = '' }) {
  const r = parse({ text, rule, unit });
  if (!r.stats.cards) {
    throw new Error('没有解析出任何可练习的英文句子，请检查文件内容或换个对齐方式。');
  }
  const meta = {
    title: (title || '未命名').trim(),
    source: 'import',
    rule: r.ruleCount,
    unit,
    createdAt: Date.now(),
    count: r.stats.cards,
    progress: { index: 0 }
  };
  return { meta, cards: r.cards, warnings: r.warnings, stats: r.stats };
}

module.exports = { readText, parse, preview, buildBook, zipPair };
