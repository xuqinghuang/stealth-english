'use strict';
// 英文/中文分句与练习单位合并（纯逻辑，可独立测试）

// 需要保护的“点”：缩写、小数、省略号 —— 分句时不能在这些地方断开
const PROTECT_RULES = [
  [/\b(?:Mr|Mrs|Ms|Dr|Prof|St|Sr|Jr|vs|etc|approx|Fig|Dept|Univ|Inc|Ltd|Co|Corp|Capt|Col|Gen|Rep|Sen|Rev|Hon)\./g,
    m => m.replace(/\./g, '\u0001')],
  [/\b(?:Jan|Feb|Mar|Apr|Jun|Jul|Aug|Sep|Sept|Oct|Nov|Dec|Mon|Tue|Wed|Thu|Fri|Sat|Sun)\./g,
    m => m.replace(/\./g, '\u0001')],
  [/\b(?:e\.g|i\.e|a\.m|p\.m)\./gi, m => m.replace(/\./g, '\u0001')],
  [/\b([A-Za-z])\.(?=\s?[A-Za-z]\.)/g, '$1\u0001'],   // U.S.A. / J. K. Rowling
  [/(\d)\.(\d)/g, '$1\u0001$2'],                      // 3.14
  [/\u2026/g, '\u0002'],
  [/\.\.\./g, '\u0002'],
  [/\.\./g, '\u0002']
];

function restore(s) {
  return s.replace(/\u0001/g, '.').replace(/\u0002/g, '...');
}

function wordCount(s) {
  return (String(s).match(/[A-Za-z0-9']+/g) || []).length;
}

// 英文分句：句末标点(+可选引号)后的空白处切断
function splitEN(text) {
  let t = String(text).replace(/\s+/g, ' ').trim();
  if (!t) return [];
  for (const [re, rep] of PROTECT_RULES) t = t.replace(re, rep);
  // 只在“下一句开头像句子”时切：大写字母/数字/引号/括号
  const raw = t.split(/(?:(?<=[.!?]["'”’)\]]?)|(?<=\u0002["'”’)\]]?))\s+(?=[A-Z0-9"'“‘（《【\u00C0-\u00DE])/);
  const arr = [];
  for (let part of raw) {
    part = restore(part.trim());
    if (part) arr.push(part);
  }
  return mergeTiny(arr, s => wordCount(s) < 2 || s.length < 8);
}

// 中文分句
function splitCN(text) {
  let t = String(text).replace(/\s+/g, ' ').trim();
  if (!t) return [];
  const parts = t.split(/(?<=[。！？；!?])/).map(s => s.trim()).filter(Boolean);
  return mergeTiny(parts, s => s.replace(/[“”，。！？；、：""''()\[\]（）0-9a-zA-Z ]/g, '').length < 2);
}

// 碎片合并：太短的句子并入相邻句
function mergeTiny(arr, isTiny) {
  const a = arr.slice();
  for (let i = 0; i < a.length; i++) {
    if (!isTiny(a[i])) continue;
    if (i + 1 < a.length) { a[i + 1] = a[i] + ' ' + a[i + 1]; a.splice(i, 1); i--; }
    else if (i > 0) { a[i - 1] += ' ' + a[i]; a.splice(i, 1); i--; }
  }
  return a;
}

// 智能合并的分组：返回每张卡由哪几句（下标）组成
// 中文必须按同一分组合并，否则合并后句数不等，配对会退化成「整段中文挂每张卡」
function smartGroups(sents, maxWords = 30) {
  const groups = [];
  let cur = [], words = 0;
  for (let i = 0; i < sents.length; i++) {
    const w = wordCount(sents[i]);
    if (cur.length && words + w > maxWords) { groups.push(cur); cur = []; words = 0; }
    cur.push(i);
    words += w;
  }
  if (cur.length) groups.push(cur);
  return groups;
}

// 智能合并：相邻短句合并成不超过 maxWords 个词的练习卡
function smartMerge(sents, maxWords = 30) {
  return smartGroups(sents, maxWords).map(g => g.map(i => sents[i]).join(' '));
}

module.exports = { splitEN, splitCN, smartMerge, smartGroups, wordCount };
