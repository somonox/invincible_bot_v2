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

## 🧠 이론적 배경 및 알고리즘 설계 (Theoretical Background)

이 프로젝트는 고속 탐색 트리 Pruning 기법과 강화학습 방법론을 결합하여 실시간 1v1 대전 Tetris 환경에서 강건한 에이전트를 구축하였습니다. 적용된 핵심 이론적 배경은 다음과 같습니다:

### 1. Zobrist 해싱 및 전치 테이블 (Zobrist Hashing & Transposition Table)
테트리스의 상태 공간은 지수적으로 넓기 때문에, 이미 방문한 상태를 재탐색하는 낭비를 방지해야 합니다.
- **Zobrist 해싱**: 각 상태 요소를 XOR 연산으로 누적하여 유일한 64비트 정수로 매핑합니다.
  \[ H(S) = \bigoplus_{y, x} K_{\text{cell}}(y, x) \oplus K_{\text{piece}}(P_{\text{curr}}) \oplus K_{\text{hold}}(P_{\text{hold}}) \oplus K_{\text{combo}}(C) \oplus \dots \]
- **전치 테이블**: 해시 충돌을 최소화하도록 버킷 형태의 테이블을 구성하고, 현재 탐색 깊이보다 얕은 탐색 정보가 이미 캐시되어 있을 경우 탐색을 조기 종료(Pruning)하여 트리 전반의 연산 속도를 대폭 단축시킵니다.

### 2. 빔 서치 프루닝 (Beam Search Pruning)
매 Ply마다 생성되는 모든 이동 수(Branching Factor $B \approx 20 \sim 30$)를 그대로 탐색하면 깊이 $D$에 따라 $O(B^D)$의 지수적 복잡도가 발생합니다.
- **알고리즘**: 각 노드에서 1-ply 휴리스틱 평가를 수행하여 점수가 높은 상위 $W$개(빔 너비, $W = 4 \sim 6$)의 후보수만 탐색 큐에 남깁니다.
- **효과**: 탐색 연산량을 선형 단위 $O(W \cdot D)$로 제한하여, 실시간 대전 환경에서도 지연 시간 없이 깊은 깊이의 탐색(4~6 lookahead)이 가능해집니다.

### 3. 정동 탐색 (Quiescence Search)
고정된 탐색 깊이(Fixed Depth Limit)에서 평가를 무조건 중단하면 바로 다음 수에 발생할 수 있는 극단적인 위기 상태나 기회 상태를 놓치는 **지평선 효과(Horizon Effect)**가 발생합니다.
- **알고리즘**: 탐색 한계 깊이에 도달했을 때, 필드 내 활성화된 콤보가 유지 중인 '불안정하고 시끄러운 상태(Loud State)'인 경우 탐색 깊이를 동적으로 최대 2단계 연장합니다.
- **효과**: 콤보 연속 클리어와 같이 공격성이 높고 수순이 중요한 상태에서 전술의 정밀도를 대폭 끌어올립니다.

### 4. 루트 불용성 가지치기 (Root Futility Pruning)
트리의 시작인 루트 노드에서 1-ply 휴리스틱 점수가 가장 높은 최고의 후보수 대비 점수차가 너무 많이 벌어지는(임계값 $\Delta = 40 \sim 150$) 후보군은 더 깊이 탐색해 보지 않아도 최선수가 될 가능성이 매우 낮습니다.
- **효과**: 가능성이 낮은 줄을 사전에 조기 탈락시켜 탐색 효율성을 극대화합니다.

### 5. 메타 정책 네트워크 (Meta-Policy Network)
고정된 정적 가중치(Static Weights)는 전술 변화(예: 수비형 플레이, 가비지 상쇄, 공격 집중 등)에 유연하지 못합니다.
- **구조**: Multi-Layer Perceptron(MLP) 기반의 메타 에이전트가 아군과 적군의 보드 높이, 구멍 개수, 쌓인 가비지 정보 등 6개의 매크로 피처를 실시간으로 받아 들여 상황 맞춤형 가중치를 동적으로 출력합니다.
- **잔차 학습(Residual Learning) 적용**: 기본 최적화 가중치를 기준점으로 두고 네트워크가 변동 델타(Delta Offset) 값을 계산하도록 구현함으로써 학습의 수렴성과 안정성을 보장합니다.

### 6. 유전 알고리즘 & CMA-ES 최적화
전통적인 경사하강법(Gradient Descent)은 테트리스의 불연속적인 보상 함수와 넓은 상태 공간에서 지역 최적점(Local Minima)에 빠지기 쉽습니다.
- **Separable CMA-ES**: 다변량 정규 분포로부터 모델 파라미터 후보군을 샘플링하여 시뮬레이션을 돌려 피트니스(Combo 달성도, 생존율)를 평가하고, 엘리트 후보들의 방향으로 분포의 평균과 분산(Covariance)을 진화(Evolution)시켜 비선형적인 조합에서도 강건하게 전술 모델을 학습시킵니다.

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

---

## 🔗 참고 (References)
* 이 프로젝트의 일부 물리 엔진 설계 및 아이디어는 [Mochbot/fusion](https://github.com/Mochbot/fusion)의 구조를 참고하여 개발되었습니다.
