import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  aliasRules,
  hasPositiveTestGate,
  platformCfgOccurrences,
  resolveModuleFile,
  rustAttributes,
  sourceOccurrences,
} from './check-libos-boundary.mjs';

test('only Rust attribute syntax is checked for platform cfg leakage', () => {
  const source = [
    'let example = "#[cfg(target_arch = \\"wasm32\\")]";',
    '// #[cfg(target_family = "wasm")]',
    '#[cfg(\n    target_family = "wasm",\n    feature = "fast-path"\n)]',
  ].join('\n');
  assert.equal(rustAttributes(source).length, 1);
  assert.equal(platformCfgOccurrences(source).length, 1);
  assert.equal(platformCfgOccurrences('let example = "#[cfg(target_arch = \\"wasm32\\") ]";').length, 0);
  assert.equal(platformCfgOccurrences('#![cfg(target_arch = "wasm32")]').length, 1);
});

test('test whitelist accepts only a positive required test cfg', () => {
  assert.equal(hasPositiveTestGate('test'), true);
  assert.equal(hasPositiveTestGate('all(test, not(target_family = "wasm"))'), true);
  assert.equal(hasPositiveTestGate('all(not(test), feature = "probe")'), false);
  assert.equal(hasPositiveTestGate('any(test, feature = "probe")'), false);
  assert.equal(hasPositiveTestGate('all(test, not(any(test, unix)))'), false);
});

test('thread and mpsc self aliases retain their platform identity', () => {
  const source = [
    'use std::thread::{self as t};',
    'use std::thread::{scope as scoped};',
    'use std::sync::mpsc::{self as mailbox};',
    'fn work() { t::scope(|_| {}); mailbox::channel::<u8>(); }',
  ].join('\n');
  const aliases = aliasRules(source);
  assert(aliases.some((item) => item.name === 't' && item.category === '平台线程'));
  assert(aliases.some((item) => item.name === 'scoped' && item.category === '平台线程'));
  assert(aliases.some((item) => item.name === 'mailbox' && item.category === '平台通道'));
  assert(sourceOccurrences(source).some((item) => item.category === '平台线程'));
  assert(sourceOccurrences(source).some((item) => item.category === '平台通道'));
  assert(sourceOccurrences('use std::thread::{scope as scoped}; scoped(|| {});').some((item) => item.category === '平台线程'));
  assert(sourceOccurrences('std::thread::scope(|_| {});').some((item) => item.category === '平台线程'));
  assert(sourceOccurrences('fn harmless() { let text = "thread::scope"; }').length === 0);
  assert(sourceOccurrences('fn harmless() { scope(|| {}); }').length === 0);
});

test('module resolution follows explicit paths and the src/session.rs child directory', () => {
  const known = new Set([
    'packages/engine/src/session/company_source_runtime_tests.rs',
    'packages/engine/src/runtime/desk/borrowed_receiver_tests.rs',
  ]);
  assert.equal(
    resolveModuleFile('packages/engine/src/session.rs', 'company_source_runtime_tests', undefined, known),
    'packages/engine/src/session/company_source_runtime_tests.rs',
  );
  assert.equal(
    resolveModuleFile('packages/engine/src/runtime/desk.rs', 'borrowed_receiver_tests', 'desk/borrowed_receiver_tests.rs', known),
    'packages/engine/src/runtime/desk/borrowed_receiver_tests.rs',
  );
  assert.notEqual(
    resolveModuleFile('packages/engine/src/runtime/desk.rs', 'borrowed_receiver_tests', 'other.rs', known),
    'packages/engine/src/runtime/desk/borrowed_receiver_tests.rs',
  );
});
