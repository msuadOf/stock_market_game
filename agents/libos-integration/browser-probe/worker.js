let wasm;
let sharedMemory;

onmessage = async ({ data }) => {
  // 单独留在 catch 之外，确保该场景观察的是未被 harness 吞掉的 WASM trap。
  if (data.type === 'hardTrap') {
    wasm.probe_hard_trap();
    postMessage({ type: 'hardTrapReturned' });
    return;
  }
  try {
    if (data.type === 'init') {
      sharedMemory = data.memory;
      wasm = (await WebAssembly.instantiate(data.module, {
        env: {
          memory: data.memory,
          engine_clock_now: () => performance.timeOrigin + performance.now(),
        },
      })).exports;
      if (data.stackTop !== undefined) wasm.__stack_pointer.value = data.stackTop;
      postMessage({ type: 'ready' });
    } else if (data.type === 'stack') {
      postMessage({ type: 'stack', top: wasm.probe_stack() });
    } else if (data.type === 'setStack') {
      wasm.__stack_pointer.value = data.top;
      postMessage({ type: 'stackSet' });
    } else if (data.type === 'sentinelSet') {
      wasm.probe_sentinel_set(data.value);
      postMessage({ type: 'sentinelSet' });
    } else if (data.type === 'sentinelGet') {
      postMessage({ type: 'sentinelGet', value: wasm.probe_sentinel_get() });
    } else if (data.type === 'prepare') {
      const ptr = wasm.probe_prepare(data.mode);
      const [session, waitWord, active] = new Uint32Array(sharedMemory.buffer, ptr, 3);
      postMessage({ type: 'prepared', ptr, session, waitWord, active });
    } else if (data.type === 'prepareGroup') {
      const ptr = wasm.probe_prepare_group(data.count);
      const pointers = new Uint32Array(sharedMemory.buffer, ptr, data.count + 1);
      const layouts = Array.from(pointers.slice(1), layoutPtr => {
        const [session, waitWord, active] = new Uint32Array(sharedMemory.buffer, layoutPtr, 3);
        return { ptr: layoutPtr, session, waitWord, active };
      });
      postMessage({ type: 'preparedGroup', ptr, count: data.count, layouts });
    } else if (data.type === 'send') {
      wasm.probe_send(data.session, data.value);
      postMessage({ type: 'sent' });
    } else if (data.type === 'disconnect') {
      wasm.probe_disconnect(data.session);
      postMessage({ type: 'disconnected' });
    } else if (data.type === 'abort') {
      wasm.probe_abort(data.session);
      postMessage({ type: 'aborted' });
    } else if (data.type === 'stop') {
      wasm.probe_stop(data.session);
      postMessage({ type: 'stopped' });
    } else if (data.type === 'receive' || data.type === 'phaseReceive') {
      const receivingType = data.type === 'receive' ? 'receiving' : 'receivingPhase';
      postMessage({ type: receivingType });
      const width = data.type === 'receive' ? 3 : 4;
      const output = wasm.probe_alloc_words(width);
      try {
        let result;
        for (let attempt = 0; attempt < 100; attempt++) {
          if (data.type === 'receive') {
            wasm.probe_receive(data.session, data.timeoutMs, output);
          } else {
            wasm.probe_phase_receive(data.session, data.phase, data.normalMs, data.longMs, output);
          }
          result = Array.from(new Uint32Array(sharedMemory.buffer, output, width));
          if (result[0] !== 5) break;
          if (attempt === 99) throw Error('LibOS verification BUSY 在 100 次异步重试后仍未收束');
          await new Promise(resolve => setTimeout(resolve, 0));
        }
        postMessage({ type: 'received', result });
      } finally {
        wasm.probe_free_words(output, width);
      }
    } else if (data.type === 'tryReceive') {
      const output = wasm.probe_alloc_words(2);
      try {
        let result;
        for (let attempt = 0; attempt < 100; attempt++) {
          wasm.probe_try_receive(data.session, output);
          result = Array.from(new Uint32Array(sharedMemory.buffer, output, 2));
          if (result[0] !== 5) break;
          if (attempt === 99) throw Error('LibOS verification BUSY 在 100 次异步重试后仍未收束');
          await new Promise(resolve => setTimeout(resolve, 0));
        }
        postMessage({ type: 'triedReceive', result });
      } finally {
        wasm.probe_free_words(output, 2);
      }
    } else if (data.type === 'finish') {
      wasm.probe_finish(data.ptr);
      postMessage({ type: 'finished' });
    } else if (data.type === 'freeWords') {
      wasm.probe_free_words(data.ptr, data.length);
      postMessage({ type: 'wordsFreed' });
    }
  } catch (error) {
    postMessage({ type: 'error', detail: String(error.stack || error) });
  }
};
