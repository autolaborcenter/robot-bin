﻿use super::{
    MsgToChassis,
    MsgToLidar::{self, *},
};
use lidar_faselase::{
    driver::{Driver, Indexer, SupervisorEventForMultiple::*, SupervisorForMultiple},
    FrameCollector, Point, D10,
};
use pm1_sdk::model::Physical;
use std::{
    f32::consts::FRAC_PI_8,
    f64::consts::PI,
    io::Write,
    net::{IpAddr, SocketAddr, UdpSocket},
    sync::mpsc::{Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

mod collision;

pub(super) fn supervisor(chassis: Sender<MsgToChassis>, mail_box: Receiver<MsgToLidar>) {
    let mut indexer = Indexer::new(2);
    let mut frame = [
        FrameCollector {
            trans: (-141, 0, PI),
            ..FrameCollector::new()
        },
        FrameCollector {
            trans: (118, 0, 0.0),
            ..FrameCollector::new()
        },
    ];
    const FILTERS: [fn(Point) -> bool; 2] = [
        |Point { len: _, dir }| {
            const DEG90: u16 = 5760 / 4;
            const DEG30: u16 = DEG90 / 3;
            (DEG30 < dir && dir <= DEG90) || ((5760 - DEG90) < dir && dir <= (5760 - DEG30))
        },
        |Point { len: _, dir }| {
            const LIMIT: u16 = 1375; // 5760 * 1.5 / 2π
            dir < LIMIT || (5760 - LIMIT) <= dir
        },
    ];

    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    let mut address = None;
    let mut send_time = Instant::now() + Duration::from_millis(100);
    let mut update_filter = 2;

    SupervisorForMultiple::<D10>::new().join(2, |e| {
        match e {
            Connected(k, lidar) => {
                eprintln!("connected: COM{}", k);
                if let Some(i) = indexer.add(k.clone()) {
                    lidar.send(FILTERS[i]);
                    if i == 0 {
                        update_filter = 1;
                    }
                    for c in &mut frame[i..] {
                        c.clear();
                    }
                }
            }
            Disconnected(k) => {
                eprintln!("disconnected: COM{}", k);
                if let Some(i) = indexer.remove(k) {
                    if i == 0 {
                        update_filter = 0;
                    }
                    for c in &mut frame[i..] {
                        c.clear();
                    }
                }
            }
            Event(k, e, s) => {
                let now = Instant::now();
                // 更新
                if let Some(j) = indexer.find(&k) {
                    if j == update_filter {
                        update_filter = 2;
                        let _ = s.send(FILTERS[j]);
                    }
                    if let Some((_, (i, s))) = e {
                        frame[j].put(i as usize, s);
                    }
                }
                // 响应请求
                while let Ok(msg) = mail_box.try_recv() {
                    match msg {
                        Check(model, predictor) => {
                            let target = predictor.target;
                            match collision::detect(&frame, model, predictor) {
                                Some((time, odometry)) => {
                                    let k = time.as_secs_f32() / 2.0;
                                    if odometry.s > 0.15 || odometry.a > FRAC_PI_8 {
                                        let _ = chassis.send(MsgToChassis::Move(Physical {
                                            speed: target.speed * k,
                                            ..target
                                        }));
                                    }
                                    println!("! {}", (k * 25.5) as u8);
                                }
                                None => {
                                    let _ = chassis.send(MsgToChassis::Move(target));
                                    println!("! 127");
                                }
                            }
                        }
                        Send(a) => address = a.map(|a| SocketAddr::new(IpAddr::V4(a), 50005)),
                    }
                }
                // 发送
                if let Some(a) = address {
                    if now >= send_time {
                        send_time = now + Duration::from_millis(100);
                        let mut buf = Vec::new();
                        let _ = buf.write_all(&[255]);
                        frame[1].write_to(&mut buf);
                        frame[0].write_to(&mut buf);
                        let _ = socket.send_to(buf.as_slice(), a);
                    }
                }
            }
            ConnectFailed { current, target } => {
                eprintln!("{}/{}", current, target);
                thread::sleep(Duration::from_secs(1));
            }
        }
        2
    });
}
