# Rspack Factorize 필사 학습 플랜

## 학습 전략: Bottom-Up (기반 인프라 → 핵심 로직 → 통합)

## 전체 흐름도

```
[repair()] entry point
    |
[ProcessDependenciesTask] groups deps by resource_id
    |
[FactorizeTask] (Background) calls ModuleFactory.create()
    |                              |
    |                   [NormalModuleFactory]
    |                     beforeResolve -> factorize hook
    |                     -> resolve_normal_module()
    |                     -> afterResolve -> createModule
    |                     -> afterFactorize
    |
[FactorizeResultTask] (Main) writes to module graph
    |
[AddTask] -> [BuildTask] -> [ProcessDependenciesTask] (cycle continues)
```

---

## Phase 1: Task Loop 시스템 (기반 인프라)

**왜 먼저?** factorize의 모든 task가 이 위에서 돌아간다. 이걸 모르면 나머지가 추상적으로만 느껴짐.

**필사 대상:**
- `rspack_core/src/utils/task_loop.rs` (182줄)

**핵심 구조:**
- `Task<Ctx>` trait: `get_task_type()`, `main_run()`, `background_run()`
- `TaskType` enum: `Main` vs `Background`
- `TaskLoop` struct: main_task_queue(VecDeque) + background_task_count
- `run_task_loop()`: Main은 순차, Background는 tokio spawn

**핵심 포인트:**
- Main task 결과는 LIFO(push_front)로 DFS - 하나의 모듈 빌드 체인을 빠르게 완료
- Background task 결과는 FIFO(push_back)
- tokio unbounded_channel로 Background → Main 결과 전달

**검증:** 테스트 코드까지 필사하여 `cargo test`로 동작 확인

---

## Phase 2: 데이터 타입 정의 (입출력 이해)

**왜 두 번째?** factorize가 뭘 받고 뭘 뱉는지 알아야 로직을 읽을 수 있다.

**필사 대상:**

### 2-1. `rspack_core/src/module_factory.rs` (91줄)
- `ModuleFactoryCreateData`: factorize의 입력
  - request, context, dependencies, issuer, resolver_factory
  - file/context/missing_dependencies (처리 중 누적)
  - diagnostics (처리 중 누적)
- `ModuleFactoryResult`: factorize의 출력 - `Option<BoxModule>`
- `ModuleFactory` trait: `async fn create()` 단 하나의 메서드

### 2-2. `rspack_core/src/dependency/factorize_info.rs` (78줄)
- `FactorizeInfo`: diagnostics + file/context/missing dependencies
- factorize 결과의 메타데이터를 dependency에 기록하는 구조
- `is_success()`: diagnostics가 비어있으면 성공

---

## Phase 3: FactorizeTask & FactorizeResultTask (핵심 로직)

**왜 세 번째?** Phase 1의 Task trait 구현체이자 Phase 2의 데이터를 실제로 사용하는 곳.

**필사 대상:**
- `rspack_core/src/compilation/build_module_graph/graph_updater/repair/factorize.rs` (226줄)

### FactorizeTask (Background task)
1. context 결정: dependency context > original_module_context > options.context
2. `ModuleFactoryCreateData` 구성
3. `module_factory.create()` 호출
4. 에러 처리: `options.bail`이면 즉시 에러, 아니면 Diagnostic으로 변환
5. `FactorizeInfo` 생성
6. `FactorizeResultTask` 반환

### FactorizeResultTask (Main task)
1. 실패 시 make_failed_dependencies에 기록
2. file/context/missing dependencies를 artifact에 기록
3. factorize_info를 각 dependency에 기록
4. 성공 시 `AddTask` 생성 (module을 graph에 추가)

---

## Phase 4: ProcessDependenciesTask (의존성 그룹핑)

**왜 네 번째?** FactorizeTask를 누가 만드는지 이해해야 전체 흐름이 연결됨.

**필사 대상:**
- `rspack_core/src/compilation/build_module_graph/graph_updater/repair/process_dependencies.rs` (117줄)

**핵심 로직:**
- `resource_identifier`로 dependencies를 그룹핑 (같은 모듈을 가리키는 dep 묶기)
- 그룹별로 `FactorizeTask` 생성
- `module_factory`는 `dependency_type`으로 조회 (dependency_factories map)

---

## Phase 5: repair() 진입점 (오케스트레이션)

**왜 다섯 번째?** 전체 파이프라인의 시작점. Phase 3-4를 알아야 의미가 읽힌다.

**필사 대상:**
- `rspack_core/src/compilation/build_module_graph/graph_updater/repair/mod.rs` (70줄)

**핵심 로직:**
1. build_dependencies를 parent_module별로 그룹핑
2. parent가 있으면 → `ProcessDependenciesTask` (기존 모듈의 하위 dependency)
3. parent가 없으면(entry) → 바로 `FactorizeTask`
4. `run_task_loop()` 호출로 전체 파이프라인 시작

---

## Phase 6: NormalModuleFactory (가장 복잡한 핵심)

**왜 마지막?** Phase 1-5를 다 이해한 뒤에 봐야 hook 시스템과 resolve 로직의 복잡도를 감당할 수 있다.

**필사 대상:**
- `rspack_core/src/normal_module_factory.rs` (~850줄)

**함수 단위로 나눠서 진행:**

### 6-1. Hook 정의 + struct (1-65줄)
- `define_hook!` 매크로로 11개 hook 선언
- `NormalModuleFactoryHooks` struct
- `NormalModuleFactory` struct (options, loader_resolver_factory, plugin_driver)

### 6-2. `create()` + `before_resolve()` (68-124줄)
- `ModuleFactory` trait 구현
- beforeResolve hook → factorize() → afterFactorize hook

### 6-3. `resolve_normal_module()` 전반부 (136-350줄)
- request 파싱: match resource, inline loader, scheme
- `!`, `!!`, `-!` prefix로 loader 제어
- module rules 매칭

### 6-4. `resolve_normal_module()` 후반부
- 실제 resolve 호출 (파일 경로 결정)
- scheme 처리 (data:, http: 등)
- afterResolve → createModule → NormalModule 생성

### 6-5. `factorize()` 메서드
- factorize hook (플러그인이 모듈 직접 반환 가능)
- resolve hook → resolve_normal_module()
- 최종 모듈 반환

---

## 프로젝트 구조

```
~/Documents/rust/factorize/
├── crates/
│   ├── task_loop/              # Phase 1
│   ├── module_factory/         # Phase 2
│   ├── factorize_task/         # Phase 3-5
│   └── normal_module_factory/  # Phase 6
```

각 phase마다 crate를 분리하면:
- 컴파일 가능한 단위로 검증하면서 진행 가능
- 의존 관계를 직접 느낄 수 있음 (task_loop -> module_factory -> factorize_task)

## 검증 방법
- 각 Phase 필사 후 `cargo check`로 컴파일 확인
- Phase 1은 테스트 코드까지 필사하여 `cargo test`로 동작 확인
- Phase 3 완료 후 간단한 mock ModuleFactory를 만들어 FactorizeTask integration test 작성
