import { preparePinnedSysroot } from '../../scripts/wasm-panic/prepare-sysroot.mjs';
if (process.argv.length !== 4) throw Error('必须提供 rustc 与新 sysroot 的绝对路径');
await preparePinnedSysroot(process.argv[2], process.argv[3]);
