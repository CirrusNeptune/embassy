use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::zerocopy_channel::{Channel, Receiver, Sender};
use static_cell::StaticCell;

use crate::consts;
use crate::leds::{Color, Keyframe};

#[derive(Copy, Clone)]
pub struct HaCommandSetEffect {
    pub entity_name: &'static str,
    pub effect_name: &'static str,
}

#[derive(Copy, Clone)]
pub enum HaCommand {
    SetEffect(HaCommandSetEffect),
}

pub struct HaButtonCommand {
    pub(crate) keyframes: &'static [Keyframe],
    pub(crate) command: HaCommand,
}

pub const BUTTON_COMMANDS: [HaButtonCommand; 16] = [
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 255, g: 141, b: 56 },
            },
            Keyframe {
                frame: 500,
                color: Color { r: 226, g: 206, b: 81 },
            },
            Keyframe {
                frame: 1000,
                color: Color { r: 131, g: 230, b: 96 },
            },
            Keyframe {
                frame: 1500,
                color: Color { r: 50, g: 227, b: 52 },
            },
            Keyframe {
                frame: 2000,
                color: Color { r: 50, g: 239, b: 163 },
            },
            Keyframe {
                frame: 2500,
                color: Color { r: 59, g: 132, b: 230 },
            },
            Keyframe {
                frame: 3000,
                color: Color { r: 98, g: 107, b: 225 },
            },
            Keyframe {
                frame: 3500,
                color: Color { r: 255, g: 141, b: 56 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Pastel Colors",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 255, g: 255, b: 255 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Daylight",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 255, g: 0, b: 0 },
            },
            Keyframe {
                frame: 500,
                color: Color { r: 0, g: 255, b: 0 },
            },
            Keyframe {
                frame: 1000,
                color: Color { r: 0, g: 0, b: 255 },
            },
            Keyframe {
                frame: 1500,
                color: Color { r: 255, g: 255, b: 0 },
            },
            Keyframe {
                frame: 2000,
                color: Color { r: 0, g: 255, b: 255 },
            },
            Keyframe {
                frame: 2500,
                color: Color { r: 255, g: 0, b: 255 },
            },
            Keyframe {
                frame: 3000,
                color: Color { r: 255, g: 0, b: 0 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Party",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 227, g: 20, b: 166 },
            },
            Keyframe {
                frame: 500,
                color: Color { r: 231, g: 58, b: 140 },
            },
            Keyframe {
                frame: 1000,
                color: Color { r: 168, g: 65, b: 232 },
            },
            Keyframe {
                frame: 1500,
                color: Color { r: 231, g: 12, b: 213 },
            },
            Keyframe {
                frame: 2000,
                color: Color { r: 227, g: 20, b: 166 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Romance",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 240, g: 143, b: 44 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Cozy",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 227, g: 57, b: 12 },
            },
            Keyframe {
                frame: 500,
                color: Color { r: 227, g: 119, b: 19 },
            },
            Keyframe {
                frame: 1000,
                color: Color { r: 226, g: 19, b: 12 },
            },
            Keyframe {
                frame: 1500,
                color: Color { r: 227, g: 57, b: 12 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Fireplace",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 166, g: 231, b: 66 },
            },
            Keyframe {
                frame: 500,
                color: Color { r: 34, g: 233, b: 67 },
            },
            Keyframe {
                frame: 1000,
                color: Color { r: 201, g: 236, b: 32 },
            },
            Keyframe {
                frame: 1500,
                color: Color { r: 166, g: 231, b: 66 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Forest",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 232, g: 95, b: 38 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Club",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 209, g: 153, b: 226 },
            },
            Keyframe {
                frame: 500,
                color: Color { r: 154, g: 136, b: 225 },
            },
            Keyframe {
                frame: 1000,
                color: Color { r: 209, g: 153, b: 226 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Spring",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 225, g: 30, b: 97 },
            },
            Keyframe {
                frame: 500,
                color: Color { r: 228, g: 46, b: 153 },
            },
            Keyframe {
                frame: 1000,
                color: Color { r: 255, g: 130, b: 103 },
            },
            Keyframe {
                frame: 1500,
                color: Color { r: 255, g: 51, b: 76 },
            },
            Keyframe {
                frame: 2000,
                color: Color { r: 225, g: 30, b: 97 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Sunset",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 53, g: 201, b: 255 },
            },
            Keyframe {
                frame: 500,
                color: Color { r: 17, g: 108, b: 224 },
            },
            Keyframe {
                frame: 1000,
                color: Color { r: 8, g: 22, b: 224 },
            },
            Keyframe {
                frame: 1500,
                color: Color { r: 0, g: 145, b: 224 },
            },
            Keyframe {
                frame: 2000,
                color: Color { r: 53, g: 201, b: 255 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Ocean",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 255, g: 243, b: 188 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Warm White",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 114, g: 108, b: 92 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Night light",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 255, g: 218, b: 228 },
            },
            Keyframe {
                frame: 500,
                color: Color { r: 255, g: 210, b: 241 },
            },
            Keyframe {
                frame: 1000,
                color: Color { r: 255, g: 218, b: 228 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Relax",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 30, g: 30, b: 133 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "TV time",
        }),
    },
    HaButtonCommand {
        keyframes: &[
            Keyframe {
                frame: 0,
                color: Color { r: 3, g: 2, b: 133 },
            },
            Keyframe {
                frame: 500,
                color: Color { r: 0, g: 69, b: 133 },
            },
            Keyframe {
                frame: 1000,
                color: Color { r: 41, g: 0, b: 133 },
            },
            Keyframe {
                frame: 1500,
                color: Color { r: 3, g: 2, b: 133 },
            },
        ],
        command: HaCommand::SetEffect(HaCommandSetEffect {
            entity_name: consts::DESK_STRIP_ENTITY,
            effect_name: "Deepdive",
        }),
    },
];

pub type CommandReceiver = Receiver<'static, ThreadModeRawMutex, Option<HaCommand>>;

pub struct CommandSender(Sender<'static, ThreadModeRawMutex, Option<HaCommand>>);

impl CommandSender {
    pub fn borrow(&mut self) -> CommandSender {
        // SAFETY: inner channel reference is 'static
        CommandSender(unsafe { &mut *(self as *mut CommandSender) }.0.borrow())
    }

    pub fn set_effect(&mut self, entity_name: &'static str, effect_name: &'static str) {
        if let Some(v) = self.0.try_send() {
            v.replace(HaCommand::SetEffect(HaCommandSetEffect {
                entity_name,
                effect_name,
            }));
            self.0.send_done();
        }
    }

    pub fn on_button_pressed(&mut self, i: usize) {
        if let Some(button_cmd) = BUTTON_COMMANDS.get(i) {
            if let Some(v) = self.0.try_send() {
                v.replace(button_cmd.command);
                self.0.send_done();
            }
        }
    }
}

pub struct CommandChannel(Channel<'static, ThreadModeRawMutex, Option<HaCommand>>);

impl CommandChannel {
    pub fn new(buf: &'static mut [Option<HaCommand>]) -> Self {
        Self(Channel::new(buf))
    }
    pub fn split(&'static mut self) -> (CommandSender, CommandReceiver) {
        let (sender, receiver) = self.0.split();
        (CommandSender(sender), receiver)
    }
}

const CHANNEL_BUF_LEN: usize = 64;

pub fn make_channel() -> &'static mut CommandChannel {
    static BUF: StaticCell<[Option<HaCommand>; CHANNEL_BUF_LEN]> = StaticCell::new();
    let buf = BUF.init([Default::default(); CHANNEL_BUF_LEN]);
    static CHANNEL: StaticCell<CommandChannel> = StaticCell::new();
    CHANNEL.init(CommandChannel::new(buf))
}

pub const ENTITIES_TO_SUBSCRIBE: [&str; 1] = [consts::DESK_STRIP_ENTITY];
