//! 2048，作为一个可安装的包。
//!
//! 四乘四的格子，四个方向的按钮，一个分数和一个「重来」。没有主机服务，没有权限，没有文件：
//! 一局的全部状态是十六个数字，一局结束之后什么也不留 —— 这是「装得上、卸得掉」这件事最
//! 干净的一个例子，也是为什么先迁它。
//!
//! # 为什么是按钮而不是滑动
//!
//! 面板是一根手指的触摸屏，而这一版 ABI 给应用的输入是「按下 / 抬起 / 移动」三个事件加上
//! 这一帧的位置 —— 判定一次滑动要包自己记住起点、算方向、定阈值，这些规则在每一局游戏里
//! 都会不一样。四个按钮把「往哪边走」变成一个点击，按钮本身就是答案，也顺手说明了格子能
//! 往哪边动。等 ABI 有了手势，按钮可以撤掉，格子不用改。
//!
//! 随机性来自一个线性同余，种子是宿主的开机时刻（`hb_now_ms`）：包里没有随机数发生器，而
//! 一台刚上电的机器，开机时刻是它身上唯一每次都不一样的东西。

#![no_std]

use app_sdk::{app, format_into, host, theme, widgets, App, Context, Event, Rect, Ui};

const SIZE: usize = 4;
const CELLS: usize = SIZE * SIZE;

/// 一次移动：把四个格子从一端读到另一端。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

struct Game {
    /// 每格是 2 的指数：0 是空，1 是 2，2 是 4，11 是 2048。
    cells: [u8; CELLS],
    score: u32,
    best: u32,
    /// 上一次移动之后有没有格子动过：没有的话不生成新格子，否则连按同一个方向会一直加分。
    moved: bool,
    over: bool,
    /// 线性同余的状态。
    seed: u32,
    /// 四个方向键里被按着的那个，为了画按下的样子。
    pressed: Option<Direction>,
}

impl Game {
    fn value(cell: u8) -> u32 {
        if cell == 0 {
            0
        } else {
            1u32 << cell
        }
    }

    fn next(&mut self) -> u32 {
        // Numerical Recipes 的那组常数，取 32 位：包里没有随机数发生器，而这一局只需要
        // 「下一格是 2 还是 4」和「空在哪一格」两件不确定。
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.seed >> 16
    }

    fn spawn(&mut self) {
        let mut empty = 0;

        for cell in self.cells {
            if cell == 0 {
                empty += 1;
            }
        }

        if empty == 0 {
            return;
        }

        let mut wanted = (self.next() % empty as u32) as usize;

        for index in 0..CELLS {
            if self.cells[index] != 0 {
                continue;
            }

            if wanted > 0 {
                wanted -= 1;
                continue;
            }

            // 十分之一是 4，跟原版一样。
            self.cells[index] = if self.next() % 10 == 0 { 2 } else { 1 };
            return;
        }
    }

    fn reset(&mut self) {
        self.cells = [0; CELLS];
        self.score = 0;
        self.moved = false;
        self.over = false;
        self.spawn();
        self.spawn();
    }

    /// 一行的四个格子，按「往 `direction` 走」的顺序读出来。
    fn line(&self, direction: Direction, index: usize) -> [u8; SIZE] {
        let mut line = [0u8; SIZE];

        for at in 0..SIZE {
            line[at] = match direction {
                Direction::Left => self.cells[index * SIZE + at],
                Direction::Right => self.cells[index * SIZE + (SIZE - 1 - at)],
                Direction::Up => self.cells[at * SIZE + index],
                Direction::Down => self.cells[(SIZE - 1 - at) * SIZE + index],
            };
        }

        line
    }

    fn write_line(&mut self, direction: Direction, index: usize, line: [u8; SIZE]) {
        for at in 0..SIZE {
            let cell = match direction {
                Direction::Left => index * SIZE + at,
                Direction::Right => index * SIZE + (SIZE - 1 - at),
                Direction::Up => at * SIZE + index,
                Direction::Down => (SIZE - 1 - at) * SIZE + index,
            };

            self.cells[cell] = line[at];
        }
    }

    /// 把一行挤到一头，相同的两个合成一个。返回这一行合成出来的分数。
    ///
    /// 合成过的格子不再参加下一次比较：`4 4 8` 往左走是 `8 8` 而不是 `16`，一排一格只合成
    /// 一次，这是原版的规则，也是这一步唯一容易写错的地方。
    fn collapse(line: &mut [u8; SIZE]) -> u32 {
        let mut packed = [0u8; SIZE];
        let mut merged = [false; SIZE];
        let mut at = 0usize;
        let mut score = 0;

        for cell in line.iter().copied() {
            if cell == 0 {
                continue;
            }

            if at > 0 && !merged[at - 1] && packed[at - 1] == cell {
                packed[at - 1] = cell + 1;
                merged[at - 1] = true;
                score += 1u32 << (cell + 1);
            } else {
                packed[at] = cell;
                at += 1;
            }
        }

        *line = packed;
        score
    }

    fn move_to(&mut self, direction: Direction) {
        let mut moved = false;
        let mut gained = 0;

        for index in 0..SIZE {
            let mut line = self.line(direction, index);
            let before = line;

            gained += Self::collapse(&mut line);

            if line != before {
                moved = true;
            }

            self.write_line(direction, index, line);
        }

        self.moved = moved;
        self.score += gained;

        if self.score > self.best {
            self.best = self.score;
        }

        if moved {
            self.spawn();
        }

        self.over = !moved && !self.any_move();
    }

    /// 还有没有一步可走：有空格，或者有两格相邻且相同。
    fn any_move(&self) -> bool {
        for index in 0..CELLS {
            if self.cells[index] == 0 {
                return true;
            }
        }

        for row in 0..SIZE {
            for column in 0..SIZE {
                let here = self.cells[row * SIZE + column];

                let right = column + 1 < SIZE && self.cells[row * SIZE + column + 1] == here;
                let down = row + 1 < SIZE && self.cells[(row + 1) * SIZE + column] == here;

                if right || down {
                    return true;
                }
            }
        }

        false
    }
}

/// 格子的颜色：越大的越暖，跟桌面上的磁贴一套颜色。
fn tile_color(cell: u8) -> u32 {
    match cell {
        0 => theme::SURFACE,
        1 => 0x003A_3A42,
        2 => 0x004A_4632,
        3 => 0x006A_4A2E,
        4 => 0x007A_3A2A,
        5 => 0x008A_2E2E,
        6 => 0x009A_2636,
        7 => 0x00A6_2A4E,
        8 => 0x008A_2A8A,
        9 => 0x005A_3A9A,
        10 => 0x003A_4A9A,
        _ => theme::ACCENT,
    }
}

impl App for Game {
    fn new(context: &Context) -> Self {
        let mut game = Self {
            cells: [0; CELLS],
            score: 0,
            best: 0,
            moved: false,
            over: false,
            seed: (host::now_ms() ^ 0x5F3A) as u32,
            pressed: None,
        };

        game.reset();
        let _ = context;

        game
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        let directions = [
            (Direction::Up, up_rect()),
            (Direction::Down, down_rect()),
            (Direction::Left, left_rect()),
            (Direction::Right, right_rect()),
        ];

        self.pressed = None;

        for (direction, rect) in directions {
            if context.input.pressing(rect) {
                self.pressed = Some(direction);
            }

            if context.tapped(rect) && !self.over {
                self.move_to(direction);
            }
        }

        if context.tapped(new_rect()) {
            self.reset();
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);

        ui.text(24.0, 28.0, "2048", 22.0, theme::MUTED);

        let mut storage = [0u8; 16];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let score = format_into!(buffer, "{}", self.score);

        widgets::reading(ui, Rect::new(200.0, 18.0, 160.0, 60.0), score, "分数", 24.0);

        // 棋盘：正方形，居中，下边留一排给方向键。
        let board = Rect::new(90.0, 84.0, 300.0, 300.0);
        widgets::card(ui, board, 0x0022_2228);

        let gap = 8.0;
        let cell_size = (board.width.min(board.height) - gap * (SIZE as f32 + 1.0)) / SIZE as f32;
        let origin_x = board.x + (board.width - (cell_size * SIZE as f32 + gap * (SIZE as f32 + 1.0))) / 2.0;
        let origin_y = board.y + gap;

        for row in 0..SIZE {
            for column in 0..SIZE {
                let cell = self.cells[row * SIZE + column];
                let rect = Rect::new(
                    origin_x + gap + column as f32 * (cell_size + gap),
                    origin_y + row as f32 * (cell_size + gap),
                    cell_size,
                    cell_size,
                );

                ui.rounded_rect(rect, 10.0, tile_color(cell));

                if cell != 0 {
                    let mut storage = [0u8; 12];
                    let mut buffer = app_sdk::text::Buf::new(&mut storage);
                    let value = format_into!(buffer, "{}", Self::value(cell));

                    ui.text_centered(rect, value, 30.0, theme::TEXT);
                }
            }
        }

        if self.over {
            ui.text_centered(board, "没有可走的一步", 28.0, theme::DANGER);
        }

        // 四个方向键：十字排列，跟它们让格子走的方向一致。
        for (direction, rect) in [
            (Direction::Up, up_rect()),
            (Direction::Down, down_rect()),
            (Direction::Left, left_rect()),
            (Direction::Right, right_rect()),
        ] {
            widgets::button(
                ui,
                rect,
                match direction {
                    Direction::Up => "↑",
                    Direction::Down => "↓",
                    Direction::Left => "←",
                    Direction::Right => "→",
                },
                26.0,
                if self.pressed == Some(direction) {
                    theme::ACCENT
                } else {
                    theme::SURFACE_HIGH
                },
                theme::TEXT,
                self.pressed == Some(direction),
            );
        }

        widgets::button(
            ui,
            new_rect(),
            "重来",
            22.0,
            theme::SURFACE_HIGH,
            theme::TEXT,
            false,
        );
    }
}

/// 四个方向键一字排开：一横排比十字省地方，而这一页剩下的地方只够一横排。
fn up_rect() -> Rect {
    Rect::new(132.0, 392.0, 108.0, 72.0)
}

fn down_rect() -> Rect {
    Rect::new(360.0, 392.0, 108.0, 72.0)
}

fn left_rect() -> Rect {
    Rect::new(18.0, 392.0, 108.0, 72.0)
}

fn right_rect() -> Rect {
    Rect::new(246.0, 392.0, 108.0, 72.0)
}

/// 「重来」在右上角，跟分数并排：它不常用，但不能没有，也不能占掉棋盘的宽度。
fn new_rect() -> Rect {
    Rect::new(372.0, 20.0, 90.0, 44.0)
}

app!(Game);
