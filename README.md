# factorize

rolldown식 구조의 학습용 번들러 — **thin JS/TS ↔ napi-rs ↔ Rust core**.
파이프라인(scan → link → generate)을 Rust core가 소유하고, JS는 옵션과 plugin 콜백만 넘긴다.

## 구조

```
packages/factorize (TS)        thin facade — factorize()/watch(), 옵션·plugin만 전달
        │ napi
crates/node_binding            BindingBundler(build, onModuleParsed, onTransform)
                               BindingWatcher(Rust-owned watch 루프)
        │
crates/factorize_core          Bundler: scan → link → generate, Plugin trait
crates/factorize_watcher       파일 감시 (FsWatcher)
```

- **scan**: entry에서 상대 import를 따라가며 module graph 구성 (worklist)
- **link**: 실행 순서 결정
- **generate**: 모듈 코드 이어붙이기
- plugin 훅(`moduleParsed`, `transform`)은 **Rust core가 napi ThreadsafeFunction으로 JS를 호출**

## 사용

### CLI
```sh
cargo run -p factorize_cli -- path/to/entry.js
```

### JS API
```sh
cd packages/factorize
npm run build:binding   # Rust → .node 생성 (코드 바꿀 때마다)
npm start               # 빌드 데모 (plugin: moduleParsed + transform)
npm run watch           # watch 데모 (파일 변경 시 Rust가 rebuild)
```

```ts
import { factorize, watch } from "./src/factorize";

// 빌드 + plugin
const build = factorize({ input: "./entry.js" }, [
  {
    moduleParsed: (id) => console.log("parsed:", id),
    transform: (code, id) => `/* ${id} */\n${code}`,
  },
]);
const out = await build.build();

// watch — 루프는 Rust가 소유, JS는 이벤트만 구독
const w = watch({ input: "./entry.js" });
w.on("bundle_end", (e) => console.log(`${e.modules.length} modules`));
w.on("change", (e) => console.log("changed:", e.path));
```

## 스코프

번들러 핵심을 최소로 구현해 **rolldown의 JS↔napi↔Rust 경계 패턴**을 익히는 게 목표.
실제 파서(oxc)·tree-shaking·code-splitting은 단순화돼 있다 — 자세한 전환 기록은 `ROLLDOWN_MIGRATION.md`.
