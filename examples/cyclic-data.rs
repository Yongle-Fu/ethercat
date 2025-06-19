use ethercat::{
    AlState, DomainIdx as DomainIndex, Master, MasterAccess, Offset, PdoCfg, PdoEntryIdx,
    PdoEntryInfo, PdoEntryPos, SlaveAddr, SlaveId, SlavePos, SmCfg,
};
use ethercat_esi::EtherCatInfo;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, prelude::*},
    time::Duration,
};
use std::time::Instant;

type BitLen = u8;

pub fn main() -> Result<(), io::Error> {
    // env_logger::init();
    // 初始化logger，设置默认日志级别为Debug
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,ethercat=debug,cyclic-data=debug")).init();
    // 或者明确指定级别
    // env_logger::Builder::new().filter_level(LevelFilter::info).init();

    // let args: Vec<_> = env::args().collect();
    let file_name = "BrainCo-Revo2Slave.xml";
    // let file_name = match args.len() {
    //     2 => &args[1],
    //     _ => {
    //         println!("usage: {} ESI-FILE", env!("CARGO_PKG_NAME"));
    //         return Ok(());
    //     }
    // };

    log::info!("Parse XML file {}", file_name);
    if !std::path::Path::new(&file_name).exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("File {} not found", file_name),
        ));
    }
    let mut esi_file = File::open(file_name)?;
    let mut esi_xml_string = String::new();
    esi_file.read_to_string(&mut esi_xml_string)?;
    let esi = EtherCatInfo::from_xml_str(&esi_xml_string)?;
    // read_esi(&esi);
    // return Ok(());

    #[allow(unreachable_code)]
    let (mut master, domain_idx, offsets) = init_master(&esi, 0_u32)?;
    for (s, o) in &offsets {
        log::info!("PDO offsets of Slave {}:", u16::from(*s));
        for (pdo, (bit_len, offset)) in o {
            log::info!(
                " - {:X}:{:X} - {:?}, bit length: {}",
                u16::from(pdo.idx),
                u8::from(pdo.sub_idx),
                offset,
                bit_len
            );
        }
    }
    const CYCLE_TIME: Duration = Duration::from_micros(50_000);
    master.activate()?;

    let entry_offsets = offsets.get(&SlavePos::from(0))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No offsets found for slave 0"))?;

    loop {
        // 在循环开始处记录时间
        let cycle_start = Instant::now();

        master.receive()?;
        master.domain(domain_idx).process()?;
        master.domain(domain_idx).queue()?;
        master.send()?;
        let m_state = master.state()?;
        let d_state = master.domain(domain_idx).state();
        log::info!("Master state: {:?}", m_state);
        log::info!("Domain state: {:?}", d_state);
        if m_state.link_up && m_state.al_states == 8 {
            let mut raw_data = master.domain_data(domain_idx)?;
            log::info!("{:?}", raw_data);

            let read = true; // 假设我们要读取数据
            let read_len: usize = 96; // 假设我们要读取96个字节
            let op_entries = vec![
                PdoEntryIdx { idx: 0x6000, sub_idx: 0x01 }, // 示例操作PDO
                // 可以添加更多的操作PDO
            ];
            let write_bytes = vec![500u16; 6]; // 示例数据

            // 使用存储的偏移量访问PDO数据
            for (entry_idx, (_bit_len, offset)) in entry_offsets {
                if !op_entries.contains(&entry_idx) {
                    continue; // 跳过非操作PDO
                }
                if read {
                    let values = read_pdo_data(&raw_data, entry_idx, &offset, read_len)
                    .expect("Failed to read PDO data");
                    // TODO: 处理读取的数据, callback
                    break; // 只处理第一个操作PDO
                }
                // write PDO数据
                write_pdo_data(&mut raw_data, entry_idx, &offset, &write_bytes)
                    .expect("Failed to write PDO data");
                // TODO: 处理写入的数据, callback, or not wait for response
                break; // 只处理第一个操作PDO
            }   
        }

        // 循环结束处检查时间并等待
        let elapsed = cycle_start.elapsed();
        if elapsed < CYCLE_TIME {
            std::thread::sleep(CYCLE_TIME - elapsed);
        } else {
            log::warn!("周期超时! 用时: {:?}, 周期时间: {:?}", elapsed, CYCLE_TIME);
        }
        // thread::sleep(cycle_time);
    }

    // 5. 清理资源
    #[allow(unreachable_code)]
    master.deactivate()?;
    // master.release()?;
}

// finger_ctrl_mode
// #define CTRL_MODE_POS_TIME   0x01  // 位置 + 时间
// #define CTRL_MODE_POS_SPD    0x02  // 位置 + 速度
// #define CTRL_MODE_SPD        0x03  // 速度控制
// #define CTRL_MODE_CURRENT    0x04  // 电流控制
// #define CTRL_MODE_PWM        0x05  // PWM占空比

// #[repr(C, packed)]
// struct RevoRxPdo {
//     multi_finger_ctrl_mode: u16, // TBD
//     finger_param1: [i16; 6],
//     finger_param2: [u16; 6],
//     single_finger_ctrl_mode: u8, // 控制模式
//     single_finger_id: u8,
//     single_finger_param1: i16,
//     single_finger_param2: u16,
// }

// #[repr(C, packed)]
// struct RevoTxPdo {
//     finger_pos: [u16; 6],
//     finger_spd: [i16; 6],
//     finger_cur: [i16; 6],
//     finger_status: [u8; 6], // 状态字
// }

// // 安全地访问PDO数据
// fn access_pdo_data(domain_data: &mut [u8]) {
//     // 获取RX PDO的写入接口
//     let rx_pdo = unsafe {
//         &mut *(domain_data.as_mut_ptr() as *mut RevoRxPdo)
//     };
    
//     // 设置控制模式
//     rx_pdo.multi_finger_ctrl_mode = 1;
//     rx_pdo.finger_param1 = [1000; 6]; // 初始化手指参数1
//     rx_pdo.finger_param2 = [200; 6]; // 初始化手指参数2
    
//     // 获取TX PDO的读取接口
//     let tx_pdo = unsafe {
//         &*(domain_data.as_ptr().add(32) as *const RevoTxPdo)
//     };
    
//     // 读取位置数据
//     let positions = unsafe {
//         std::slice::from_raw_parts(
//             tx_pdo.finger_pos.as_ptr() as *const i32,
//             3  // 假设有3个手指位置
//         )
//     };
    
//     println!("手指位置: {:?}", positions);
// }

fn read_pdo_data(
    domain_data: &[u8],
    entry: &PdoEntryIdx,
    offset: &Offset,
    byte_len: usize,
) -> Result<Vec<u8>, anyhow::Error> {
    let (offset, _) = (offset.byte, offset.bit);
    let values = domain_data.get(offset..offset + byte_len).ok_or(anyhow::anyhow!("Invalid offset"))?;
    log::debug!("读取 Entry 0x{:#?}:{:?} = {:?}", entry.idx, entry.sub_idx, values);
    Ok(values.to_vec())
}

fn write_pdo_data(
    domain_data: &mut [u8],
    entry: &PdoEntryIdx,
    offset: &Offset,
    bytes: &[u8],
) -> Result<(), anyhow::Error> {
    let (offset, _) = (offset.byte, offset.bit);
    domain_data[offset..offset + bytes.len()].copy_from_slice(bytes);
    log::debug!("写入 Entry 0x{:#?}:{:?} = {:?}", entry.idx, entry.sub_idx, &bytes);
    Ok(())
}

// fn process_pdo_data(
//     master: &mut Master,
//     domain_idx: usize,
//     entry_offsets: &[(EntryIndex, (u32, Offset))],
//     r_or_w: bool,
//     count: usize,
//     bytes: &[u8],
// ) -> Result<(), PdoError> {
//     let byte_len = (*bit_len as usize + 7) / 8;
//     match r_or_w {
//         true => read_raw_bytes(data_slice, entry_idx, offset, byte_len)?,
//         false => {
//             let start = i * byte_len;
//             write_raw_bytes(data_slice, entry_idx, offset, bytes.get(start..start + byte_len), byte_len)?
//         }
//     }
// }






type SlaveMap = HashMap<SlavePos, HashMap<PdoEntryIdx, (BitLen, Offset)>>;

pub fn init_master(
    esi: &EtherCatInfo,
    idx: u32,
) -> Result<(Master, DomainIndex, SlaveMap), io::Error> {
    let mut master = Master::open(idx, MasterAccess::ReadWrite)?;
    log::info!("Reserve master");
    master.reserve()?;
    log::info!("Create domain");
    let domain_idx = master.create_domain()?;
    let mut offsets: HashMap<SlavePos, HashMap<PdoEntryIdx, (u8, Offset)>> = HashMap::new();

    for (dev_nr, dev) in esi.description.devices.iter().enumerate() {
        let slave_pos = SlavePos::from(dev_nr as u16);
        log::info!("Request PreOp state for {:?}", slave_pos);
        master.request_state(slave_pos, AlState::PreOp)?;
        let slave_info = master.get_slave_info(slave_pos)?;
        log::info!("Found device {}:{:?}", dev.name, slave_info);
        let slave_addr = SlaveAddr::ByPos(dev_nr as u16);
        let slave_id = SlaveId {
            vendor_id: esi.vendor.id,
            product_code: dev.product_code,
        };
        let mut config = master.configure_slave(slave_addr, slave_id)?;
        let mut entry_offsets: HashMap<PdoEntryIdx, (u8, Offset)> = HashMap::new();

        let rx_pdos: Vec<PdoCfg> = dev
            .rx_pdo
            .iter()
            .map(|pdo| PdoCfg {
                idx: pdo.idx,
                entries: pdo
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| PdoEntryInfo {
                        entry_idx: e.entry_idx,
                        bit_len: e.bit_len as u8,
                        name: e.name.clone().unwrap_or_default(),
                        pos: PdoEntryPos::from(i as u8),
                    })
                    .collect(),
            })
            .collect();

        let tx_pdos: Vec<PdoCfg> = dev
            .tx_pdo
            .iter()
            .map(|pdo| PdoCfg {
                idx: pdo.idx,
                entries: pdo
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| PdoEntryInfo {
                        entry_idx: e.entry_idx,
                        bit_len: e.bit_len as u8,
                        name: e.name.clone().unwrap_or_default(),
                        pos: PdoEntryPos::from(i as u8),
                    })
                    .collect(),
            })
            .collect();

        let output = SmCfg::output(2.into());
        let input = SmCfg::input(3.into());

        config.config_sm_pdos(output, &rx_pdos)?;
        config.config_sm_pdos(input, &tx_pdos)?;

        for pdo in &rx_pdos {
            // Positions of RX PDO
            log::info!("Positions of RX PDO 0x{:X}:", u16::from(pdo.idx));
            for entry in &pdo.entries {
                let offset = config.register_pdo_entry(entry.entry_idx, domain_idx)?;
                entry_offsets.insert(entry.entry_idx, (entry.bit_len, offset));
            }
        }
        for pdo in &tx_pdos {
            // Positions of TX PDO
            log::info!("Positions of TX PDO 0x{:X}:", u16::from(pdo.idx));
            for entry in &pdo.entries {
                let offset = config.register_pdo_entry(entry.entry_idx, domain_idx)?;
                entry_offsets.insert(entry.entry_idx, (entry.bit_len, offset));
            }
        }

        let cfg_index = config.index();
        let cfg_info = master.get_config_info(cfg_index)?;
        log::info!("Config info: {:#?}", cfg_info);
        if cfg_info.slave_position.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Unable to configure slave",
            ));
        }
        offsets.insert(slave_pos, entry_offsets);
    }
    Ok((master, domain_idx, offsets))
}

pub fn read_esi(
    esi: &EtherCatInfo
) {
    // let domain_idx = DomainIndex::from(0);
    let mut offsets: HashMap<SlavePos, HashMap<PdoEntryIdx, (u8, Offset)>> = HashMap::new();
    for (dev_nr, dev) in esi.description.devices.iter().enumerate() {
        let slave_pos = SlavePos::from(dev_nr as u16);
        log::info!("Request PreOp state for {:?}", slave_pos);
        // master.request_state(slave_pos, AlState::PreOp)?;
        // let slave_info = master.get_slave_info(slave_pos)?;
        // log::info!("Found device {}:{:?}", dev.name, slave_info);
        log::info!("Configuring device {} at position {}", dev.name, dev_nr);
        // let slave_addr = SlaveAddr::ByPos(dev_nr as u16);
        // let slave_id = SlaveId {
        //     vendor_id: esi.vendor.id,
        //     product_code: dev.product_code,
        // };
        // let mut config = master.configure_slave(slave_addr, slave_id)?;
        let entry_offsets: HashMap<PdoEntryIdx, (u8, Offset)> = HashMap::new();

        let rx_pdos: Vec<PdoCfg> = dev
            .rx_pdo
            .iter()
            .map(|pdo| PdoCfg {
                idx: pdo.idx,
                entries: pdo
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| PdoEntryInfo {
                        entry_idx: e.entry_idx,
                        bit_len: e.bit_len as u8,
                        name: e.name.clone().unwrap_or_default(),
                        pos: PdoEntryPos::from(i as u8),
                    })
                    .collect(),
            })
            .collect();

        let tx_pdos: Vec<PdoCfg> = dev
            .tx_pdo
            .iter()
            .map(|pdo| PdoCfg {
                idx: pdo.idx,
                entries: pdo
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| PdoEntryInfo {
                        entry_idx: e.entry_idx,
                        bit_len: e.bit_len as u8,
                        name: e.name.clone().unwrap_or_default(),
                        pos: PdoEntryPos::from(i as u8),
                    })
                    .collect(),
            })
            .collect();

        // let output = SmCfg::output(2.into());
        // let input = SmCfg::input(3.into());
        // config.config_sm_pdos(output, &rx_pdos)?;
        // config.config_sm_pdos(input, &tx_pdos)?;

        for pdo in &rx_pdos {
            // Positions of RX PDO
            log::info!("Positions of RX PDO 0x{:X}:", u16::from(pdo.idx));
            for entry in &pdo.entries {
                log::info!("Registering RX PDO entry {:} {:#?} with bit length {}", entry.name, entry.entry_idx, entry.bit_len);
                // let offset = config.register_pdo_entry(entry.entry_idx, domain_idx)?;
                // entry_offsets.insert(entry.entry_idx, (entry.bit_len, offset));
            }
        }
        for pdo in &tx_pdos {
            // Positions of TX PDO
            log::info!("Positions of TX PDO 0x{:X}:", u16::from(pdo.idx));
            for entry in &pdo.entries {
                // let offset = config.register_pdo_entry(entry.entry_idx, domain_idx)?;
                // entry_offsets.insert(entry.entry_idx, (entry.bit_len, offset));
                log::info!("Registering TX PDO entry {:} {:#?} with bit length {}", entry.name, entry.entry_idx, entry.bit_len);
            }
        }

        // let cfg_index = config.index();
        // let cfg_info = master.get_config_info(cfg_index)?;
        // log::info!("Config info: {:#?}", cfg_info);
        // if cfg_info.slave_position.is_none() {
        //     return Err(io::Error::new(
        //         io::ErrorKind::Other,
        //         "Unable to configure slave",
        //     ));
        // }
        offsets.insert(slave_pos, entry_offsets);
    }
    // Ok((master, domain_idx, offsets))
}