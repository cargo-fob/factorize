use std::{
    any::Any,
    collections::VecDeque,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

// rspack은 rspack_error::Result를 쓰지만, 여기선 anyhow 없이 표준 에러로 대체
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Task가 반환하는 결과 - 다음에 실행할 task들의 목록
pub type TaskResult<Ctx> = Result<Vec<Box<dyn Task<Ctx>>>>;

/// Task 타입: Main(순차) vs Background(병렬)
pub enum TaskType {
    Main,
    Background,
}

/// 모든 task가 구현해야 하는 trait
///
/// rspack 원본은 `Debug + Send + Any + AsAny`를 요구하지만
/// 여기선 AsAny 대신 Any만 사용
///
/// TODO: 아래 trait을 채워 넣으세요
/// 힌트:
///   - get_task_type(): TaskType을 반환
///   - main_run(): Main task일 때 실행 (context 접근 가능)
///   - background_run(): Background task일 때 실행 (context 접근 불가)
#[async_trait::async_trait]
pub trait Task<Ctx>: Debug + Send + Any {
    fn get_task_type(&self) -> TaskType;

    async fn main_run(self: Box<Self>, _context: &mut Ctx) -> TaskResult<Ctx> {
        unreachable!();
    }

    async fn background_run(self: Box<Self>) -> TaskResult<Ctx> {
        unreachable!();
    }
}

/// Task Loop의 핵심 구조체
///
/// TODO: 아래 필드들의 역할을 이해하고 구현을 채워 넣으세요
///
/// 핵심 설계:
///   - main_task_queue: Main task를 순차 실행하기 위한 큐 (VecDeque)
///   - background_task_count: 현재 실행 중인 background task 수
///   - is_expected_shutdown: 에러 발생 시 background task가 결과를 보내지 않도록 하는 플래그
///   - task_result_sender/receiver: background task → main loop 결과 전달 채널
struct TaskLoop<Ctx> {
    main_task_queue: VecDeque<Box<dyn Task<Ctx>>>,
    background_task_count: u32,
    is_expected_shutdown: Arc<AtomicBool>,
    task_result_sender: UnboundedSender<TaskResult<Ctx>>,
    task_result_receiver: UnboundedReceiver<TaskResult<Ctx>>,
}

impl<Ctx: 'static + Send> TaskLoop<Ctx> {
    fn new(init_main_tasks: Vec<Box<dyn Task<Ctx>>>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel::<TaskResult<Ctx>>();
        Self {
            main_task_queue: VecDeque::from(init_main_tasks),
            is_expected_shutdown: Arc::new(AtomicBool::new(false)),
            background_task_count: 0,
            task_result_sender: tx,
            task_result_receiver: rx,
        }
    }

    /// 메인 루프
    ///
    /// TODO: 아래 로직을 구현하세요
    ///
    /// 동작 순서:
    ///   1. init_background_tasks를 모두 spawn
    ///   2. loop:
    ///      a. try_recv()로 완료된 background 결과를 drain (to_front=false)
    ///      b. main_task_queue에서 task 하나 pop
    ///      c. task가 없고 background도 0이면 → return Ok(())
    ///      d. task가 없고 background가 있으면 → recv().await로 대기
    ///      e. task가 있으면 → main_run() 실행 (to_front=true, DFS!)
    async fn run_task_loop(
        &mut self,
        ctx: &mut Ctx,
        init_background_tasks: Vec<Box<dyn Task<Ctx>>>,
    ) -> Result<()> {
        // TODO: 구현하세요
        todo!("run_task_loop 구현")
    }

    /// task 결과를 처리하여 새 task들을 큐에 넣는다
    ///
    /// TODO: 아래 로직을 구현하세요
    ///
    /// `to_front` 파라미터가 핵심:
    ///   - true (Main task 결과): push_front → LIFO/DFS
    ///     → 하나의 모듈 빌드 체인(Factorize→Add→Build)을 빠르게 완료
    ///   - false (Background task 결과): push_back → FIFO/BFS
    ///
    /// 에러 발생 시: is_expected_shutdown을 true로 설정하고 에러 반환
    fn handle_task_result(&mut self, result: TaskResult<Ctx>, to_front: bool) -> Result<()> {
        // TODO: 구현하세요
        todo!("handle_task_result 구현")
    }

    /// Background task를 tokio로 spawn
    ///
    /// TODO: 아래 로직을 구현하세요
    ///
    /// 핵심:
    ///   - background_task_count 증가
    ///   - tokio::spawn으로 background_run() 실행
    ///   - 완료 후 is_expected_shutdown 체크 → false면 tx.send()
    ///
    /// rspack 원본은 rspack_tasks::spawn_in_compiler_context를 쓰지만
    /// 여기선 tokio::spawn으로 대체
    fn spawn_background(&mut self, task: Box<dyn Task<Ctx>>) {
        // TODO: 구현하세요
        todo!("spawn_background 구현")
    }
}

/// 외부에 노출되는 진입점
///
/// init_tasks를 Background/Main으로 분류한 뒤 TaskLoop 실행
pub async fn run_task_loop<Ctx: 'static + Send>(
    ctx: &mut Ctx,
    init_tasks: Vec<Box<dyn Task<Ctx>>>,
) -> Result<()> {
    let (background_tasks, main_tasks) = init_tasks
        .into_iter()
        .partition(|task| matches!(task.get_task_type(), TaskType::Background));
    let mut task_loop = TaskLoop::new(main_tasks);
    task_loop.run_task_loop(ctx, background_tasks).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // === 테스트용 Context ===
    #[derive(Default)]
    struct Context {
        call_sync_task_count: u32,
        max_sync_task_call: u32,
        sync_return_error: bool,
        async_return_error: bool,
    }

    // === Main Task (SyncTask) ===
    // 호출될 때마다 count를 증가시키고, max에 도달하지 않았으면 AsyncTask 2개를 생성
    //
    // TODO: Task<Context> trait을 구현하세요
    #[derive(Debug)]
    struct SyncTask;

    #[async_trait::async_trait]
    impl Task<Context> for SyncTask {
        fn get_task_type(&self) -> TaskType {
            TaskType::Main
        }

        async fn main_run(self: Box<Self>, context: &mut Context) -> TaskResult<Context> {
            // TODO: 구현하세요
            // 힌트:
            //   - sync_return_error가 true면 에러 반환
            //   - call_sync_task_count 증가
            //   - max_sync_task_call 미만이면 AsyncTask 2개 반환
            //   - 그 외에는 빈 vec 반환
            todo!()
        }
    }

    // === Background Task (AsyncTask) ===
    // 10ms sleep 후 SyncTask 하나를 반환
    //
    // TODO: Task<Context> trait을 구현하세요
    #[derive(Debug)]
    struct AsyncTask {
        async_return_error: bool,
    }

    #[async_trait::async_trait]
    impl Task<Context> for AsyncTask {
        fn get_task_type(&self) -> TaskType {
            TaskType::Background
        }

        async fn background_run(self: Box<Self>) -> TaskResult<Context> {
            // TODO: 구현하세요
            // 힌트:
            //   - 10ms sleep
            //   - async_return_error가 true면 에러 반환
            //   - 아니면 SyncTask 하나를 담은 vec 반환
            todo!()
        }
    }

    // === 테스트 ===
    // 정상 동작: AsyncTask(1) → SyncTask(1) → AsyncTask(2) → SyncTask(2) → ...
    // max_sync_task_call=4일 때 call_sync_task_count가 7이 되는 이유를 추적해보세요
    //
    //   SyncTask 1회 → AsyncTask 2개 spawn
    //   SyncTask 2회 → AsyncTask 2개 spawn
    //   SyncTask 3회 → AsyncTask 2개 spawn
    //   SyncTask 4회 → count == max, 빈 vec 반환
    //   남은 AsyncTask들이 SyncTask를 반환 → 5, 6, 7회
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_run_task_loop() {
        // Test 1: 정상 동작
        let mut context = Context {
            call_sync_task_count: 0,
            max_sync_task_call: 4,
            sync_return_error: false,
            async_return_error: false,
        };
        let res = run_task_loop(
            &mut context,
            vec![Box::new(AsyncTask {
                async_return_error: false,
            })],
        )
        .await;
        assert!(res.is_ok(), "task loop should run successfully");
        assert_eq!(context.call_sync_task_count, 7);

        // Test 2: sync 에러
        let mut context = Context {
            call_sync_task_count: 0,
            max_sync_task_call: 4,
            sync_return_error: true,
            async_return_error: false,
        };
        let res = run_task_loop(
            &mut context,
            vec![Box::new(AsyncTask {
                async_return_error: false,
            })],
        )
        .await;
        assert!(res.is_err(), "should return sync error");
        assert_eq!(context.call_sync_task_count, 0);

        // Test 3: async 에러
        let mut context = Context {
            call_sync_task_count: 0,
            max_sync_task_call: 4,
            sync_return_error: false,
            async_return_error: true,
        };
        let res = run_task_loop(
            &mut context,
            vec![Box::new(AsyncTask {
                async_return_error: false,
            })],
        )
        .await;
        assert!(res.is_err(), "should return async error");
        assert_eq!(context.call_sync_task_count, 1);
    }
}
