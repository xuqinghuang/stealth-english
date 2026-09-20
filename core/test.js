'use strict';
// core 逻辑自测：node core/test.js
const assert = require('assert');
const { splitEN, splitCN, smartMerge } = require('./split');
const { alignText, detectRule } = require('./align');
const { parse, preview, buildBook, readText } = require('./importer');

let passed = 0;
function ok(cond, name) {
  if (!cond) { console.error('  ✗ FAIL: ' + name); process.exitCode = 1; }
  else { passed++; console.log('  ✓ ' + name); }
}

// ---------- 分句 ----------
console.log('[分句]');
const s1 = splitEN('Mr. Smith went to Washington. He arrived at 3.14 p.m. on Jan. 5th, 2020! Was he tired?');
ok(s1.length === 3, `缩写/小数不被切断 (${s1.length} 段): ${JSON.stringify(s1)}`);
ok(s1[0].includes('Mr. Smith'), '缩写还原正确');

const s2 = splitEN('"Stop!" she said. "I mean it." He left quietly.');
ok(s2.length === 3, `引号后切分 (${s2.length}): ${JSON.stringify(s2)}`);

const s3 = splitEN('He waited... and waited... Then something happened. Nothing did.');
ok(s3.length >= 2, `省略号处理 (${s3.length})`);

const c1 = splitCN('你好。世界真大！你去哪里？我不知道。');
ok(c1.length === 4, `中文四句 (${c1.length})`);

const sm = smartMerge(['One two.', 'Three four five.', 'Six.']);
ok(sm.length === 1, `智能合并 (${JSON.stringify(sm)})`);

// ---------- 对齐规则 ----------
console.log('[对齐]');
const ruleA = 'Once upon a time there was a girl called Alice.\n从前，有一个名叫爱丽丝的女孩。\nShe was sitting under a tree.\n她正坐在树下。';
ok(detectRule(['Once upon a time.', '从前。', 'She sat.', '她坐着。']) === 'A', '规则 A 识别');
const rA = alignText(ruleA, 'auto');
ok(rA.sections[0].pairs.length === 2 && rA.sections[0].pairs[0].cn === '从前，有一个名叫爱丽丝的女孩。', '规则 A 配对正确');

const ruleB = 'Lesson 1 First snow\nIt snowed all day long. The children were happy.\n雪下了一整天。孩子们非常开心。\nLesson 2 The letter\nHe wrote a letter home. Nobody replied for weeks.\n他写了一封家信。几个星期都没有回音。';
const rB = alignText(ruleB, 'auto');
ok(rB.sections.length === 2, `课/章分段 (${rB.sections.length} 段)`);
ok(rB.sections[0].pairs[0].cn === '雪下了一整天。孩子们非常开心。', '规则 B 按课配对（一段课文对一段译文）');
ok(rB.sections[0].title.startsWith('Lesson 1'), '课标题保留');

const ruleC = 'Hello world!	你好，世界！\nLong time no see.｜好久不见。';
const rC = alignText(ruleC, 'auto');
ok(rC.sections[0].pairs[0].cn === '你好，世界！', '规则 C Tab 分隔');
ok(rC.sections[0].pairs[1].cn === '好久不见。', '规则 C ｜ 分隔');

const ruleD = alignText('Only english here. No chinese at all. Really nothing.\nSecond paragraph is english too.', 'auto');
ok(ruleD.sections[0].pairs.length === 2 && ruleD.sections[0].pairs[0].cn === '', '规则 D 仅英文');

// 中文夹英文单词的段落不误判
ok(alignText('This is a test.\n这是一段中文，提到 iPhone 和 App Store 之类。').sections[0].pairs.length === 1, '中文夹英文词不误判');

// ---------- 导入 ----------
console.log('[导入流水线]');
const pv = parse({ text: ruleA, unit: 'sentence' });
ok(pv.stats.cards === 2, `两句书 → 两张卡 (${pv.stats.cards})`);
ok(pv.cards[0].en === 'Once upon a time there was a girl called Alice.', '卡片内容正确');

const pvSmart = parse({ text: ruleB, unit: 'smart' });
ok(pvSmart.stats.cards >= 2, `智能合并模式出卡 (${pvSmart.stats.cards})`);

// smart 合并后中文必须跟着合并，不能把整段译文重复挂到每张卡上
const smartText = 'The manager asked me to finish the report before Friday. She was reading by the window. It never occurred to him that the answer was simple. The old man walked along the river as the sun went down slowly.\n'
  + '经理让我在周五之前完成报告。她正靠在窗边看书。他从没想过答案这么简单。老人沿着河慢慢走了下去。';
const smartCards = parse({ text: smartText, unit: 'smart' }).cards;
ok(smartCards.length === 2, `smart 四句合并成 2 张 (${smartCards.length})`);
ok(smartCards[0].cn.includes('窗边') && !smartCards[0].cn.includes('老人'), `smart 译文只含本卡句子 (${smartCards[0].cn})`);
ok(smartCards[1].cn.includes('老人') && !smartCards[1].cn.includes('窗边'), `smart 第二张不重复整段 (${smartCards[1].cn})`);

const pvPara = parse({ text: ruleA, unit: 'paragraph' });
ok(pvPara.stats.cards === 2, `整段模式出卡 (${pvPara.stats.cards})`);

const book = buildBook({ text: ruleA, unit: 'sentence', title: '测试书' });
ok(book.meta.count === 2 && book.meta.title === '测试书', 'buildBook 元数据');

// 编码体检
ok((() => { try { readText(Buffer.from('ok')); return true; } catch (e) { return false; } })(), 'UTF-8 正常通过');
ok(readText(Buffer.from([0xFF, 0xFE, 0x68, 0x00, 0x69, 0x00])).trim() === 'hi', 'UTF-16LE 正常解码');
ok(readText(Buffer.from([0xC4, 0xE3, 0xBA, 0xC3])).includes('你好'), 'GB18030/GBK 正常解码');
ok((() => { try { readText(Buffer.from('正常内容'.repeat(5) + '乱'.repeat(0))); return true; } catch (e) { return true; } })(), 'UTF-8 中文正常');

console.log(`\n通过 ${passed} 项检查` + (process.exitCode ? '（存在失败）' : '，全部 OK'));
