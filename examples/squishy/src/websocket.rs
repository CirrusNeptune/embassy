use defmt::{assert, debug, unwrap};
use edge_ws::FrameHeader;
use embassy_futures::select;
use embassy_net::tcp::{Error, TcpSocket};
use embassy_net::IpEndpoint;
use embassy_time::{Duration, Instant, Timer};
use embedded_io_async::Write;
use ufmt::uwrite;

use crate::command::{CommandReceiver, HaCommand, ENTITIES_TO_SUBSCRIBE};
use crate::consts::HA_CONSTS;
use crate::leds::LedSender;

const PING_INTERVAL: u64 = 30;

fn map_edge_ws_error<R>(result: Result<R, edge_ws::io::Error<Error>>) -> Result<R, Error> {
    match result {
        Ok(r) => Ok(r),
        Err(edge_ws::Error::Io(e)) => Err(e),
        _ => Err(Error::ConnectionReset),
    }
}

enum ReadWsOk {
    Ok,
    Discard,
}

macro_rules! make_send_function {
    ($name:ident, $debug:expr, $format:expr) => {
        async fn $name(&mut self) -> Result<(), Error> {
            debug!($debug);
            let mut s = heapless::String::<256>::new();
            uwrite!(s, $format, self.id).unwrap();
            self.id += 1;
            self.send_text_payload(&s).await
        }
    };
}

macro_rules! make_send_function_1parm {
    ($name:ident, $debug:expr, $format:expr) => {
        async fn $name(&mut self, parm: &str) -> Result<(), Error> {
            debug!($debug);
            let mut s = heapless::String::<256>::new();
            uwrite!(s, $format, parm, self.id).unwrap();
            self.id += 1;
            self.send_text_payload(&s).await
        }
    };
}

macro_rules! make_send_function_2parm {
    ($name:ident, $debug:expr, $format:expr) => {
        async fn $name(&mut self, parm1: &str, parm2: &str) -> Result<(), Error> {
            debug!($debug);
            let mut s = heapless::String::<256>::new();
            uwrite!(s, $format, parm1, parm2, self.id).unwrap();
            self.id += 1;
            self.send_text_payload(&s).await
        }
    };
}

pub struct Websocket<'a, const PAYLOAD_BUF_LEN: usize> {
    socket: TcpSocket<'a>,
    payload_buffer: &'a mut heapless::Vec<u8, PAYLOAD_BUF_LEN>,
    id: i32,
    authenticated: bool,
    last_received_instant: Instant,
    receiver: &'a mut CommandReceiver,
    led_sender: &'a mut LedSender,
}

impl<'a, const PAYLOAD_BUF_LEN: usize> Websocket<'a, PAYLOAD_BUF_LEN> {
    pub fn new(
        socket: TcpSocket<'a>,
        payload_buffer: &'a mut heapless::Vec<u8, PAYLOAD_BUF_LEN>,
        receiver: &'a mut CommandReceiver,
        led_sender: &'a mut LedSender,
    ) -> Self {
        Self {
            socket,
            payload_buffer,
            id: 1,
            authenticated: false,
            last_received_instant: Instant::MIN,
            receiver,
            led_sender,
        }
    }

    async fn read_each_http_header_line<F: Fn(&str)>(&mut self, f: F) -> Result<(), Error> {
        let mut concat_vec = heapless::Vec::<u8, 512>::new();
        let mut cr = false;

        while self
            .socket
            .read_with(|bytes| {
                let mut line_start = 0_usize;
                for (i, elem) in bytes.iter().enumerate() {
                    match *elem {
                        b'\n' => {
                            assert!(cr);

                            let line_end = i - 1;
                            if line_start == line_end {
                                return (i + 1, false);
                            }

                            if !concat_vec.is_empty() {
                                unwrap!(concat_vec.extend_from_slice(&bytes[line_start..line_end]));
                                f(core::str::from_utf8(concat_vec.as_slice()).unwrap());
                                concat_vec.clear();
                            } else {
                                f(core::str::from_utf8(&bytes[line_start..line_end]).unwrap());
                            }

                            line_start = i + 1;
                            cr = false;
                        }
                        b'\r' => {
                            assert!(!cr);
                            cr = true;
                        }
                        _ => {
                            assert!(!cr);
                        }
                    }
                }

                unwrap!(concat_vec.extend_from_slice(&bytes[line_start..]));
                (bytes.len(), true)
            })
            .await?
        {}

        Ok(())
    }

    async fn read_ws_frame_header(&mut self) -> Result<FrameHeader, Error> {
        let mut header_buf = heapless::Vec::<u8, { FrameHeader::MAX_LEN }>::new();
        loop {
            if let Some(header) = self
                .socket
                .read_with(|bytes_mut| {
                    let bytes: &[u8] = bytes_mut;
                    let (deserialize_bytes, start_header_buf_len) = if !header_buf.is_empty() {
                        let start_header_buf_len = header_buf.len();
                        unwrap!(header_buf.extend_from_slice(bytes));
                        (header_buf.as_slice(), start_header_buf_len)
                    } else {
                        (bytes, 0_usize)
                    };
                    match FrameHeader::deserialize(&deserialize_bytes) {
                        Ok((header, payload_offset)) => (payload_offset - start_header_buf_len, Some(header)),
                        Err(edge_ws::Error::Incomplete(_)) => {
                            unwrap!(header_buf.extend_from_slice(bytes));
                            (bytes.len(), None)
                        }
                        _ => {
                            panic!("invalid ws header")
                        }
                    }
                })
                .await?
            {
                return Ok(header);
            }
        }
    }

    async fn read_ws_payload(&mut self, header: &FrameHeader) -> Result<ReadWsOk, Error> {
        self.payload_buffer.clear();
        let payload_len = header.payload_len as usize;
        if payload_len == 0 {
            return Ok(ReadWsOk::Ok);
        }
        if payload_len <= self.payload_buffer.capacity() {
            while self.payload_buffer.len() < payload_len {
                self.socket
                    .read_with(|bytes| {
                        let read_size = usize::min(bytes.len(), payload_len - self.payload_buffer.len());
                        let payload_buf_start = self.payload_buffer.len();
                        unwrap!(self.payload_buffer.extend_from_slice(&bytes[0..read_size]));
                        header.mask(&mut self.payload_buffer[payload_buf_start..], payload_buf_start);
                        (read_size, ())
                    })
                    .await?;
            }
            Ok(ReadWsOk::Ok)
        } else {
            debug!("discarding {} payload bytes", payload_len);
            let mut rem_discard = payload_len;
            while rem_discard > 0 {
                self.socket
                    .read_with(|bytes| {
                        let read_size = usize::min(bytes.len(), rem_discard);
                        rem_discard -= read_size;
                        (read_size, ())
                    })
                    .await?;
            }
            Ok(ReadWsOk::Discard)
        }
    }

    async fn send_ping(&mut self) -> Result<(), Error> {
        debug!("sending ping");
        const PING_HEADER: FrameHeader = FrameHeader {
            frame_type: edge_ws::FrameType::Ping,
            payload_len: 0,
            mask_key: None,
        };
        map_edge_ws_error(PING_HEADER.send(&mut self.socket).await)?;
        self.last_received_instant = Instant::now();
        Ok(())
    }

    async fn send_pong(&mut self) -> Result<(), Error> {
        debug!("sending pong");
        const PONG_HEADER: FrameHeader = FrameHeader {
            frame_type: edge_ws::FrameType::Pong,
            payload_len: 0,
            mask_key: None,
        };
        map_edge_ws_error(PONG_HEADER.send(&mut self.socket).await)
    }

    async fn send_auth(&mut self) -> Result<(), Error> {
        debug!("sending auth");
        const AUTH_HEADER: FrameHeader = FrameHeader {
            frame_type: edge_ws::FrameType::Text(false),
            payload_len: HA_CONSTS.auth.len() as u64,
            mask_key: None,
        };
        map_edge_ws_error(AUTH_HEADER.send(&mut self.socket).await)?;
        map_edge_ws_error(
            AUTH_HEADER
                .send_payload(&mut self.socket, HA_CONSTS.auth.as_bytes())
                .await,
        )
    }

    async fn send_text_payload<const N: usize>(&mut self, s: &heapless::String<N>) -> Result<(), Error> {
        debug!("< {}", s);
        let header: FrameHeader = FrameHeader {
            frame_type: edge_ws::FrameType::Text(false),
            payload_len: s.len() as u64,
            mask_key: None,
        };
        map_edge_ws_error(header.send(&mut self.socket).await)?;
        map_edge_ws_error(header.send_payload(&mut self.socket, s.as_bytes()).await)
    }

    make_send_function!(
        send_event_subscribe,
        "sending event subscribe",
        r#"{{"type":"subscribe_events","event_type":"state_changed","id":{}}}"#
    );
    make_send_function_1parm!(
        send_entity_subscribe,
        "sending entity subscribe",
        r#"{{"type":"subscribe_entities","entity_ids":"{}","id":{}}}"#
    );

    make_send_function_1parm!(
        send_turn_on,
        "sending turn on",
        r#"{{"type":"call_service","domain":"light","service":"turn_on","service_data":{{"entity_id":"{}"}},"id":{}}}"#
    );
    make_send_function_1parm!(
        send_turn_off,
        "sending turn on",
        r#"{{"type":"call_service","domain":"light","service":"turn_off","service_data":{{"entity_id":"{}"}},"id":{}}}"#
    );
    make_send_function_2parm!(
        send_set_effect,
        "sending set effect",
        r#"{{"type":"call_service","domain":"light","service":"turn_on","service_data":{{"entity_id":"{}","effect":"{}"}},"id":{}}}"#
    );

    async fn connect_socket<T: Into<IpEndpoint>>(&mut self, endpoint: T, hostname: &str) -> Result<(), Error> {
        unwrap!(self.socket.connect(endpoint).await);

        debug!("sending request");
        self.socket
            .write_all(
                "GET /api/websocket HTTP/1.1\r\n\
             Host: "
                    .as_ref(),
            )
            .await?;

        self.socket.write_all(hostname.as_ref()).await?;

        self.socket
            .write_all(
                "\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: x3JJHMbDL1EzLkh9GBhXDw==\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n"
                    .as_ref(),
            )
            .await?;

        self.read_each_http_header_line(|line| {
            debug!("{}", line);
        })
        .await
    }

    fn try_to_parse_state(led_sender: &mut LedSender, str: &str) {
        let mut parsed: Option<(&str, &str)> = None;
        let mut try_parse_effect = |name_start: usize| {
            if let Some(mut name_end) = str[name_start..].find('"') {
                name_end += name_start;
                let entity_name = &str[name_start..name_end];
                if let Some(mut effect_key_start) = str[name_end..].find(r#""effect":""#) {
                    effect_key_start += name_end;
                    let effect_start = effect_key_start + 10;
                    if let Some(mut effect_end) = str[effect_start..].find('"') {
                        effect_end += effect_start;
                        let effect_name = &str[effect_start..effect_end];
                        parsed = Some((entity_name, effect_name))
                    }
                }
            }
        };
        if let Some(start) = str.find(r#""a":{""#) {
            let name_start = start + 6;
            try_parse_effect(name_start);
        } else if let Some(start) = str.find(r#""new_state":{"entity_id":""#) {
            let name_start = start + 26;
            try_parse_effect(name_start);
        }
        if let Some((entity_name, effect_name)) = parsed {
            debug!("parsed state change {} {}", entity_name, effect_name);
            if ENTITIES_TO_SUBSCRIBE.contains(&entity_name) {
                led_sender.on_effect_changed(entity_name, effect_name);
            }
        }
    }

    async fn websocket_read(&mut self) -> Result<bool, Error> {
        let header = map_edge_ws_error(FrameHeader::recv(&mut self.socket).await)?;
        match header.frame_type {
            edge_ws::FrameType::Text(fragmented) => {
                debug!("Text frame fragmented={} len={}", fragmented, header.payload_len);
            }
            edge_ws::FrameType::Binary(fragmented) => {
                debug!("Binary frame fragmented={} len={}", fragmented, header.payload_len);
            }
            edge_ws::FrameType::Ping => {
                debug!("Ping frame len={}", header.payload_len);
            }
            edge_ws::FrameType::Pong => {
                debug!("Pong frame len={}", header.payload_len);
            }
            edge_ws::FrameType::Close => {
                debug!("Close frame len={}", header.payload_len);
            }
            edge_ws::FrameType::Continue(is_final) => {
                debug!("Continue frame final={} len={}", is_final, header.payload_len);
            }
        }

        if let ReadWsOk::Ok = self.read_ws_payload(&header).await? {
            match header.frame_type {
                edge_ws::FrameType::Text(false) => {
                    let str = core::str::from_utf8(self.payload_buffer.as_slice()).unwrap();
                    debug!("> {}", str);

                    if str.starts_with(r#"{"type":"auth_required","#) {
                        self.send_auth().await?;
                    } else if str.starts_with(r#"{"type":"auth_ok","#) {
                        debug!("authenticated");
                        self.send_event_subscribe().await?;
                        for entity in ENTITIES_TO_SUBSCRIBE {
                            self.send_entity_subscribe(entity).await?;
                        }
                        self.authenticated = true;
                    } else {
                        Self::try_to_parse_state(self.led_sender, str);
                    }
                }
                edge_ws::FrameType::Ping => {
                    self.send_pong().await?;
                }
                edge_ws::FrameType::Close => {
                    return Ok(false);
                }
                _ => {}
            }
        }

        self.last_received_instant = Instant::now();
        Ok(true)
    }

    async fn send_command(&mut self, command: &HaCommand) -> Result<(), Error> {
        match command {
            HaCommand::SetEffect(effect) => {
                self.send_set_effect(effect.entity_name, effect.effect_name).await?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Future which is ready as long as something is present in the receive buffer.
    async fn poll_read(&mut self) -> Result<(), Error> {
        self.socket.read_with(|_| (0, ())).await
    }

    async fn websocket_pump(&mut self) -> Result<bool, Error> {
        if !self.authenticated {
            // Cannot send anything until authentication is confirmed
            if !self.websocket_read().await? {
                return Ok(false);
            }
        } else {
            // Wait until we receive either socket data or an app command
            match select::select(self.socket.read_with(|_| (0, ())), self.receiver.receive()).await {
                select::Either::First(result) => {
                    // Socket has received at least one byte
                    result?;
                    if !self.websocket_read().await? {
                        return Ok(false);
                    }
                }
                select::Either::Second(command) => {
                    // App command
                    self.send_command(&command).await?;
                }
            }
        }
        Ok(true)
    }

    async fn websocket_loop(&mut self) -> Result<(), Error> {
        loop {
            let ping_timeout = Timer::at(self.last_received_instant + Duration::from_secs(PING_INTERVAL));
            match select::select(ping_timeout, self.websocket_pump()).await {
                select::Either::First(_) => {
                    self.send_ping().await?;
                }
                select::Either::Second(result) => {
                    if !result? {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn close_socket(&mut self) {
        debug!("closing");
        self.authenticated = false;
        {
            const CLOSE_HEADER: FrameHeader = FrameHeader {
                frame_type: edge_ws::FrameType::Close,
                payload_len: 2,
                mask_key: None,
            };
            CLOSE_HEADER.send(&mut self.socket).await.unwrap();
            CLOSE_HEADER
                .send_payload(&mut self.socket, &1000_u16.to_be_bytes())
                .await
                .unwrap();
        }
        self.socket.close();
        loop {
            match self.socket.read_with(|bytes| (bytes.len(), ())).await {
                Err(Error::ConnectionReset) => {
                    debug!("tcp closed");
                    break;
                }
                _ => {}
            }
        }
    }

    pub async fn run(&mut self, endpoint: IpEndpoint, hostname: &str) {
        if let Ok(_) = self.connect_socket(endpoint, hostname).await {
            self.websocket_loop().await.ok();
        }

        self.close_socket().await;
    }
}
