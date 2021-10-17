﻿use path_tracking::Tracker;
use rtk_ins570::{Enu, WGS84};
use std::{
    io::Write,
    net::UdpSocket,
    sync::{Arc, Mutex},
    thread,
};

macro_rules! write {
    ($what:expr => $where:expr) => {
        $where.write_all(std::slice::from_raw_parts(
            $what as *const _ as *const u8,
            std::mem::size_of_val($what),
        ))
    };
}

fn main() {
    let address = Arc::new(Mutex::new(None));
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:49999").unwrap());
    {
        let address = address.clone();
        let socket = socket.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 1500];
            while let Ok((_, a)) = socket.recv_from(&mut buf) {
                *address.lock().unwrap() = Some(a);
            }
        });
    }

    let controller = Tracker::new("path").unwrap();
    let mut path: Option<(String, Vec<u8>)> = None;
    let mut line = String::new();
    loop {
        line.clear();
        match std::io::stdin().read_line(&mut line) {
            Err(_) => break,
            Ok(_) => {
                let word = line.trim();
                if let Some((name, p)) = path.as_ref() {
                    if name == word {
                        if let Some(ref a) = *address.lock().unwrap() {
                            println!("send: {:?}", socket.send_to(&p, a));
                        } else {
                            eprintln!("Unity not found.");
                        }
                        continue;
                    }
                }
                match controller.read(word) {
                    Ok(p) => {
                        const HEAD: &[u8; 12] = b"gps routing\0";
                        let len = p.len() as u16;
                        let mut buf = Vec::with_capacity(HEAD.len() + len as usize * 8);

                        unsafe {
                            let _ = write!(HEAD => buf);
                            for p in p.into_iter() {
                                let WGS84 {
                                    latitude,
                                    longitude,
                                    altitude: _,
                                } = WGS84::from_enu(
                                    Enu {
                                        e: p.translation.vector[0] as f64,
                                        n: p.translation.vector[1] as f64,
                                        u: 0.0,
                                    },
                                    WGS84 {
                                        latitude: 39_9931403,
                                        longitude: 116_3281766,
                                        altitude: 0,
                                    },
                                );
                                let _ = write!(&latitude => buf);
                                let _ = write!(&longitude => buf);
                            }
                        }

                        if let Some(ref a) = *address.lock().unwrap() {
                            println!("send: {:?}", socket.send_to(&buf, a));
                        } else {
                            eprintln!("Unity not found.");
                        }

                        path = Some((word.into(), buf));
                    }
                    Err(e) => {
                        eprintln!("Failed to open file: {:?}", e);
                    }
                };
            }
        }
    }
}