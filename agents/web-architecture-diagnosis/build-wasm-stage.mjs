// 使用产品受控标准库、线程链接配置和固定工具链构建隔离诊断绑定。
import {withFrontendWasmSysroot} from '../../scripts/frontend-wasm-sysroot.mjs';
import {spawn} from 'node:child_process';
const env={...process.env,PATH:`/Users/asuad/.cargo/bin:/opt/homebrew/bin:${process.env.PATH}`,RUSTUP_TOOLCHAIN:'nightly-2026-09-05',CARGO_BUILD_JOBS:'8',CARGO_TARGET_DIR:process.cwd()+'/target/wasm-diagnostic'};
const command={env};
await withFrontendWasmSysroot(command, async ({env})=>{await new Promise((resolve,reject)=>{const p=spawn('/opt/homebrew/bin/wasm-pack',['build','--target','web','--release','--out-dir','pkg'],{cwd:'agents/web-architecture-diagnosis/wasm-stage',env,stdio:'inherit'});p.on('error',reject);p.on('close',c=>c===0?resolve():reject(Error(`wasm-pack exit ${c}`)));});},{environment:env, resolveRustc:async()=>"/Users/asuad/.rustup/toolchains/nightly-2026-09-05-aarch64-apple-darwin/bin/rustc"});
