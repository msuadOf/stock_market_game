import {test} from 'node:test';
import assert from 'node:assert/strict';
import {failureDetail} from './failure-detail.mjs';

test('保留首错及全部嵌套清理错误', () => {
  const error = new AggregateError([
    new AggregateError([new Error('运行失败'), new Error('关闭浏览器失败')], '首轮失败'),
    new Error('删除链接失败'),
  ], '清理失败');
  const detail = failureDetail(error);
  assert.equal(detail.errors[0].errors[0].message, '运行失败');
  assert.equal(detail.errors[0].errors[1].message, '关闭浏览器失败');
  assert.equal(detail.errors[1].message, '删除链接失败');
});
