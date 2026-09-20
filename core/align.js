'use strict';
// 中英对齐：规则 A 逐段交替 / B 两段式 / C 行内对照 / D 仅英文
// 输入整篇文本 → 输出 sections（课/章分段 + 每段内的 en/cn 配对）

const { splitEN, splitCN } = require('./split');

const LESSON_EN_RE = /^(chapter|lesson|text|part|unit|story|book)\s*[:.\-]?\s*(\d{1,3}|[ivxlcdm]{1,7}|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)\b/i;
const LESSON_CN_RE = /^第\s*[0-9０-９一二三四五六七八九十百千两]+\s*[课章节讲部回]/;

function isLessonHeader(p) {
  const t = String(p).trim();
  if (!t || t.length > 60) return false;
  if (LESSON_EN_RE.test(t) || LESSON_CN_RE.test(t)) return true;
  if (/^[-=_*#~—…·]{4,}$/.test(t)) return true;   // 分隔线
  return false;
}

// 判断段落语言：en / cn / mixed / other
function scriptOf(p) {
  const cjk = (p.match(/[\u4e00-\u9fff]/g) || []).length;
  const latin = (p.match(/[A-Za-z]/g) || []).length;
  if (cjk >= 2 && latin >= 3) {
    const iLat = p.search(/[A-Za-z]/);
    const iCjk = p.search(/[\u4e00-\u9fff]/);
    if (iLat < iCjk) {
      // 英文开头且中文收尾 → “英文…中文…”的行内对照行
      if (/[\u4e00-\u9fff\u3002\uff01\uff1f\uff1b\u2026]\s*$/.test(p)) return 'mixed';
      return 'en';   // 英文段里夹了几个中文词
    }
    return 'cn';     // 中文开头 → 中文段（夹英文单词不影响）
  }
  if (cjk >= 2) return 'cn';
  if (latin >= 3) return 'en';
  return 'other';
}

// 规则 C：一行里中英都有，按分隔符拆开
function splitInline(line) {
  const seps = ['\t', '｜', '|', '//', ' # ', '＃'];
  for (const sep of seps) {
    const idx = line.indexOf(sep);
    if (idx > 0 && idx < line.length - 1) {
      return { en: line.slice(0, idx).trim(), cn: line.slice(idx + sep.length).trim() };
    }
  }
  const m = line.match(/[\u4e00-\u9fff]/);
  if (m && m.index > 3) return { en: line.slice(0, m.index).trim(), cn: line.slice(m.index).trim() };
  return { en: line, cn: '' };
}

// 把段落序列按指定规则配对
function pairByRule(paras, rule) {
  const warnings = [];
  if (rule === 'C') return { pairs: paras.map(splitInline), warnings };
  if (rule === 'D') return { pairs: paras.map(p => ({ en: p, cn: '' })), warnings };

  const seq = paras.map(p => ({ p, s: scriptOf(p) })).filter(it => it.s === 'en' || it.s === 'cn');
  const enArr = seq.filter(it => it.s === 'en').map(it => it.p);
  const cnArr = seq.filter(it => it.s === 'cn').map(it => it.p);

  if (rule === 'A') {
    const pairs = [];
    let i = 0;
    while (i < seq.length) {
      const a = seq[i], b = seq[i + 1];
      if (b && a.s !== b.s) {
        pairs.push(a.s === 'en' ? { en: a.p, cn: b.p } : { en: b.p, cn: a.p });
        i += 2;
      } else {
        if (a.s === 'en') pairs.push({ en: a.p, cn: '' });
        else pairs.push({ en: '', cn: a.p });
        i += 1;
      }
    }
    return { pairs, warnings };
  }

  // 规则 B（也是兜底）：按序号一一对应
  const n = Math.min(enArr.length, cnArr.length);
  const pairs = [];
  for (let i = 0; i < n; i++) pairs.push({ en: enArr[i], cn: cnArr[i] });
  if (enArr.length > cnArr.length) {
    for (let i = n; i < enArr.length; i++) pairs.push({ en: enArr[i], cn: '' });
    warnings.push(`英文段（${enArr.length}）比中文段（${cnArr.length}）多 ${enArr.length - cnArr.length} 段，多出的段落没有配到中文`);
  } else if (cnArr.length > enArr.length) {
    warnings.push(`中文段（${cnArr.length}）比英文段（${enArr.length}）多 ${cnArr.length - enArr.length} 段，多出的中文未使用`);
  }
  return { pairs, warnings };
}

// 自动探测规则
function detectRule(paras) {
  const used = paras.map(p => scriptOf(p)).filter(s => s !== 'other');
  const en = used.filter(s => s === 'en').length;
  const cn = used.filter(s => s === 'cn').length;
  const mixed = used.filter(s => s === 'mixed').length;
  if (used.length && mixed / used.length >= 0.5) return 'C';   // 行内对照优先判定
  if (en === 0 && cn === 0) return 'empty';
  if (cn === 0) return 'D';
  // 块状（英文段连一片 + 中文段连一片）优先于交替：
  // “一段课文 + 一段译文内含多段”的双语 txt 就是这种
  if (isBlockSeq(used)) return 'B';
  let same = 0;
  for (let i = 0; i + 1 < used.length; i++) if (used[i] === used[i + 1]) same++;
  if (used.length >= 2 && same <= Math.max(1, Math.floor(used.length * 0.15))) return 'A';
  return 'B';
}

// 序列是否为 en…en cn…cn 或 cn…cn en…en（至多一次转换）
function isBlockSeq(seq) {
  let flipped = 0;
  for (let i = 1; i < seq.length; i++) if (seq[i] !== seq[i - 1]) flipped++;
  return flipped <= 1;
}

// 按空行分段落（保留原始段落），识别课/章标题
function sectionsFromText(text) {
  const paras = String(text).replace(/\r\n?/g, '\n').split('\n').map(s => s.trim()).filter(Boolean);
  const sections = [];
  let cur = { title: '', paras: [] };
  for (const p of paras) {
    if (isLessonHeader(p)) {
      if (cur.paras.length || cur.title) sections.push(cur);
      cur = { title: p, paras: [] };
    } else {
      cur.paras.push(p);
    }
  }
  if (cur.paras.length || cur.title) sections.push(cur);
  return sections;
}

// 主入口：整篇文本 → 配好对的 sections
function alignText(text, rule = 'auto') {
  const sections = sectionsFromText(text);
  const warnings = [];
  const ruleCount = {};
  const out = [];
  for (const sec of sections) {
    if (!sec.paras.length) {
      if (sec.title) out.push({ title: sec.title, pairs: [] });
      continue;
    }
    const r = (rule === 'auto') ? detectRule(sec.paras) : rule;
    if (r === 'empty') { out.push({ title: sec.title, pairs: [] }); continue; }
    const res = pairByRule(sec.paras, r);
    ruleCount[r] = (ruleCount[r] || 0) + 1;
    for (const w of res.warnings) warnings.push((sec.title ? `【${sec.title}】` : '') + w);
    out.push({ title: sec.title, pairs: res.pairs });
  }
  return { sections: out, warnings, ruleCount };
}

module.exports = { alignText, scriptOf, detectRule, isLessonHeader };
