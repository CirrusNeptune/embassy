use core::cmp::min;
use defmt::{debug, warn, error, Format, Formatter, unwrap};
use embassy_net::udp::{UdpMetadata, UdpSocket};
use num::FromPrimitive;
use nom::{Err, IResult, bytes::complete::{tag, take}, branch::alt, sequence::{tuple, preceded}, combinator::{map, map_res, map_opt}, number::complete::{le_u16, u8}, Parser, Needed, Slice};
use heapless::Vec;
use nom::error::{error_to_u32, ErrorKind};
use embassy_futures::select;
use embassy_futures::select::Either;
use ufmt::uwrite;
use crate::color::Color;
use crate::leds;
use crate::leds::{Effect, LedSender, NUM_LEDS};

fn get_led_sender() -> LedSender {
    unsafe { leds::LED_CHANNEL.sender() }
}

enum ListenCmd {
    SetColorList = 0,
    ShiftColor = 1,
    SetPrimaryColor = 2,
    SetEffect = 3,
    SetEffectSpeed = 4,
    SetBrightness = 5,
}

fn parse_color_list(input: &[u8]) -> IResult<&[u8], [Color; NUM_LEDS]> {
    let (input, color_count) = u8(input)?;
    let num_color_bytes = color_count as usize * 4;
    map(take(num_color_bytes), |color_bytes: &[u8]| {
        let mut colors = [Color::BLACK; NUM_LEDS];
        for i in 0..min(NUM_LEDS, color_bytes.len() / 4) {
            colors[i] = Color::from_rgbw(color_bytes[i * 4],
                                         color_bytes[i * 4 + 1],
                                         color_bytes[i * 4 + 2],
                                         color_bytes[i * 4 + 3]);
        }
        colors
    })(input)
}

fn parse_color(input: &[u8]) -> IResult<&[u8], Color> {
    map(take(4usize), |color_bytes: &[u8]| {
        Color::from_rgbw(color_bytes[0],
                         color_bytes[1],
                         color_bytes[2],
                         color_bytes[3])
    })(input)
}

fn parse_effect(input: &[u8]) -> IResult<&[u8], Effect> {
    map_opt(u8, Effect::from_u8)(input)
}

fn parse_set_color_list(input: &[u8]) -> IResult<&[u8], ()> {
    preceded(
        tag([ListenCmd::SetColorList as u8]),
        map(parse_color_list, |color_list| get_led_sender().set_color_list(color_list))
    )(input)
}

fn parse_shift_color(input: &[u8]) -> IResult<&[u8], ()> {
    preceded(
        tag([ListenCmd::ShiftColor as u8]),
        map(parse_color, |color| get_led_sender().shift_color(color))
    )(input)
}

fn parse_set_primary_color(input: &[u8]) -> IResult<&[u8], ()> {
    preceded(
        tag([ListenCmd::SetPrimaryColor as u8]),
        map(parse_color, |color| get_led_sender().set_primary_color(color))
    )(input)
}

fn parse_set_effect(input: &[u8]) -> IResult<&[u8], ()> {
    preceded(
        tag([ListenCmd::SetEffect as u8]),
        map(parse_effect, |effect| get_led_sender().set_effect(effect))
    )(input)
}

fn parse_set_effect_speed(input: &[u8]) -> IResult<&[u8], ()> {
    preceded(
        tag([ListenCmd::SetEffectSpeed as u8]),
        map(le_u16, |effect_speed| get_led_sender().set_effect_speed(effect_speed))
    )(input)
}

fn parse_set_brightness(input: &[u8]) -> IResult<&[u8], ()> {
    preceded(
        tag([ListenCmd::SetBrightness as u8]),
        map(u8, |brightness| get_led_sender().set_brightness(brightness))
    )(input)
}

fn parse_cmd(input: &[u8]) -> IResult<&[u8], ()> {
    alt((
        parse_set_color_list,
        parse_shift_color,
        parse_set_primary_color,
        parse_set_effect,
        parse_set_effect_speed,
        parse_set_brightness,
    ))(input)
}

fn fmt_err(err: Err<nom::error::Error<&[u8]>>) {
    match err {
        Err::Incomplete(Needed::Size(u)) => error!("Parsing requires {} bytes/chars", u),
        Err::Incomplete(Needed::Unknown) => error!("Parsing requires more data"),
        Err::Failure(c) => error!("Parsing Failure: error {} at: {}", error_to_u32(&c.code), c.input),
        Err::Error(c) => error!("Parsing Error: error {} at: {}", error_to_u32(&c.code), c.input),
    }
}

fn on_cmd_datagram_received(mut buffer: &[u8], endpoint: UdpMetadata) {
    debug!("Received datagram of {} octets", buffer.len());
    while buffer.len() > 0 {
        match parse_cmd(buffer) {
            Ok((buf, _)) => buffer = buf,
            Err(e) => {
                fmt_err(e);
                break
            },
        };
    }
}

pub async fn run<'a>(cmd_socket: &mut UdpSocket<'a>, discover_socket: &mut UdpSocket<'a>, mac: &[u8; 6]) -> ! {
    loop {
        match select::select(
            cmd_socket.recv_with(|buffer, endpoint| {
                on_cmd_datagram_received(buffer, endpoint);
            }),
            discover_socket.recv_with(|buffer, endpoint| {
                if buffer == "mow sconce discover".as_bytes() {
                    debug!("Received valid discover packet from {}", endpoint);
                    Some(endpoint)
                } else {
                    debug!("Discarding invalid discover packet from {}", endpoint);
                    None
                }
            }),
        ).await {
            Either::Second(Some(endpoint)) => {
                debug!("Sending discover reply to {}", endpoint);
                let mut reply = heapless::String::<36>::new();
                uwrite!(reply, "mow sconce reply: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]).unwrap();
                discover_socket.send_with(reply.len(), endpoint, |buffer| {
                    buffer.copy_from_slice(reply.as_bytes());
                }).await.ok();
            }
            _ => {}
        }
    }
}
