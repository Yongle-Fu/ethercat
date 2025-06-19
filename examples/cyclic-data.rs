use ethercat::{
    AlState, DomainIdx as DomainIndex, Master, MasterAccess, Offset, PdoCfg, PdoEntryIdx,
    PdoEntryInfo, PdoEntryPos, SlaveAddr, SlaveId, SlavePos, SmCfg,
};
use ethercat_esi::EtherCatInfo;
use std::{
    collections::HashMap,
    // env,
    fs::File,
    io::{self, prelude::*},
    thread,
    time::Duration,
};

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
    let cycle_time = Duration::from_micros(50_000);
    master.activate()?;

    loop {
        master.receive()?;
        master.domain(domain_idx).process()?;
        master.domain(domain_idx).queue()?;
        master.send()?;
        let m_state = master.state()?;
        let d_state = master.domain(domain_idx).state();
        log::info!("Master state: {:?}", m_state);
        log::info!("Domain state: {:?}", d_state);
        if m_state.link_up && m_state.al_states == 8 {
            let raw_data = master.domain_data(domain_idx);
            log::info!("{:?}", raw_data);
        }
        thread::sleep(cycle_time);
    }
}

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