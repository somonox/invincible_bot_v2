# Invincible Bot v2 🤖 (4-Wide Tetris RL Bot)

TETR.IO 및 로컬 환경에서 작동하는 고성능 4-Wide 테트리스 강화학습(RL) 봇 및 시뮬레이터입니다.  
초고속으로 탐색을 수행하는 **Rust 기반의 AI 시뮬레이터 코어 및 로컬 GUI**와, TETR.IO 커스텀 룸에서 플레이할 수 있는 **TypeScript/Bun 룸 클라이언트**로 구성되어 있습니다.

---

## 🚀 주요 특징 (Features)

1. **초고속 AI 탐색 (Lookahead Engine)**
   - **BFS 패스파인더 및 평면 인덱싱**: Heap Allocation을 완전히 배제한 스택 메모리 기반 캐시 배열 및 링 큐를 사용하여, 4-Ply~5-Ply 탐색 속도를 **2ms 내외**로 유지합니다.
   - **전치 테이블 (Transposition Table) & Zobrist Hashing**: Zobrist Hashing을 구현하여 동일 상태를 캐싱하고 중복 노드 탐색을 획기적으로 줄였습니다.
2. **비동기 멀티스레딩 GUI**
   - AI 탐색 연산을 GUI 렌더링 스레드와 분리하여 로컬 GUI가 **60 FPS** 고정으로 끊김 없이 쾌적하게 렌더링됩니다.
3. **완벽한 TETR.IO 물리 엔진 & SRS Parity**
   - **SRS 회전 및 Off-center Pivot 일치**: `I` 미노를 포함한 모든 블록의 회전 중심(Pivot) 좌표와 벽차기(SRS Kicks) 로직을 TETR.IO 사양과 100% 매칭하였습니다.
   - **가비지 상쇄 및 상승 시뮬레이션**: 상대방으로부터 온 가비지를 감지하여, 이를 상쇄(Cancellation)하거나 하단에서 가비지가 올라오는 현상을 예측하여 생존력을 극대화합니다.
4. **4-Wide 최적화 콤보 유지 및 PC 유도**
   - **콤보 가중치 극대화**: 콤보가 유지되는 최선의 경로를 항상 최우선으로 선택합니다.
   - **Perfect Clear (PC) 가중치**: 필드가 완전히 비는 시나리오를 감지하여 10줄의 공격을 보낼 수 있는 PC 찬스를 지능적으로 유도합니다.
5. **안정적인 입력 스케줄러**
   - **1프레임 간격 개별 입력**: 연속 동작이 한 프레임에 뭉개지지 않도록 키 이벤트 사이에 안정적인 프레임 Gap을 확보하여 입력 유실을 방지합니다.
   - **중복 탐색 차단 (Lock-based Planning)**: 미노가 완전히 락킹(Lock)된 후 다음 미노의 연산을 시작하도록 방지하여, 공중에서 미노가 겹쳐 탑아웃되는 오류를 제거했습니다.
6. **4-Wide 자동 감지 및 Spectate 모드**
   - TETR.IO 룸의 가로 너비 설정을 감지하여 가로가 4칸이 아닐 때는 자동으로 관전(Spectator) 상태로 대기하며, 4칸(4-Wide 전용)으로 설정되면 즉시 플레이어로 참가합니다.

---

## 📂 프로젝트 구조

```
├── src/
│   ├── engine/           # 테트리스 코어 물리 및 상태 관리 (board, state, movegen 등)
│   ├── rl/               # RL 에이전트 가중치, Zobrist 해시, 탐색 알고리즘
│   ├── gui/              # egui 기반의 로컬 시뮬레이터 화면 및 시각화
│   ├── lib.rs            # Rust 라이브러리 엔트리
│   ├── main.rs           # 로컬 GUI 실행 엔트리
│   └── triangle_adapter.rs # stdin/stdout JSON 통신을 위한 CLI 어댑터 엔트리
├── tetrio-bot/           # TypeScript / Bun 기반의 TETR.IO 클라이언트
│   ├── index.ts          # 봇 실행 스크립트 (teto 라이브러리 및 어댑터 연동)
│   ├── package.json
│   └── tsconfig.json
├── Cargo.toml            # Rust 의존성 및 빌드 설정
└── README.md
```

---

## 🛠️ 실행 방법 (How to Run)

### 1. 로컬 GUI 시뮬레이터 실행 (Rust)

로컬에서 봇의 탐색 과정과 시뮬레이션을 GUI 환경에서 직접 시청하고 테스트할 수 있습니다. 두 가지 버전이 제공됩니다:

* **통합 대시보드 (싱글플레이어 / 트레이닝 / 배틀 통합)**:
  ```bash
  cargo run --release --bin four_wide_bot
  ```

* **1v1 봇 대전 전용 아레나 (두 봇의 4-Wide 난타전 관전 전용)**:
  ```bash
  cargo run --release --bin battle-gui
  ```

### 2. TETR.IO 봇 클라이언트 실행 (Bun / TS)

TETR.IO 서버에 로그인하여 커스텀 멀티플레이어 룸에서 봇을 구동합니다.

#### 사전 요구사항:
- [Rust](https://www.rust-lang.org/) 및 Cargo 설치
- [Bun](https://bun.sh/) 설치

#### 단계별 실행:

1. **Rust 어댑터 바이너리 빌드**
   ```bash
   cargo build --release
   ```

2. **TETR.IO 봇 디렉토리 이동 및 의존성 설치**
   ```bash
   cd tetrio-bot
   bun install
   ```

3. **환경 변수 파일 생성**
   `tetrio-bot` 폴더 내에 `.env` 파일을 생성하고 아래 형식을 채워 넣습니다:
   ```env
   BOT_USERNAME=your_tetrio_bot_username
   BOT_PASSWORD=your_tetrio_bot_password
   ```

4. **봇 실행**
   ```bash
   bun run index.ts
   ```

   봇이 성공적으로 로그인하면 TETR.IO 커스텀 룸 초대를 대기합니다. 초대 시 가로 너비가 `4`인 방에서 실시간으로 6-ply 탐색 4-wide 플레이를 보여줍니다.
