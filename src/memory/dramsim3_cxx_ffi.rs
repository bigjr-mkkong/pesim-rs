#[cxx::bridge]
pub mod dramsim3_ffi {

    /*
     * NOTE
     * Order in this struct MATTERS
     * This is because rust code need to respect cpp data layout
     * DO NOT CHANGE it unless necessary
     */
    pub struct dramsim3_mem_event {
        time: u64,
        addr: u64,
        is_write: bool,
    }

    pub struct local_addr_bulk {
        channel: u64,
        rank: u64,
        bank_group: u64,
        bank: u64,
        row: u64,
        column: u64,
        global_addr: u64,
        bank_local_addr: u64,
    }

    unsafe extern "C++" {
        include!("rust-ffi/ms_ffi.h");

        type dramsim3_ext;

        pub fn take_events(self: Pin<&mut dramsim3_ext>) -> Vec<dramsim3_mem_event>;

        pub fn create_sim(config_file: &str, output_dir: &str) -> UniquePtr<dramsim3_ext>;

        pub fn ClockTick(self: Pin<&mut dramsim3_ext>);

        pub fn GetTCK(self: Pin<&mut dramsim3_ext>) -> f64;
        pub fn GetBusBits(self: Pin<&mut dramsim3_ext>) -> i32;
        pub fn GetBurstLength(self: Pin<&mut dramsim3_ext>) -> i32;
        pub fn GetQueueSize(self: Pin<&mut dramsim3_ext>) -> i32;
        pub fn GetClock(self: Pin<&mut dramsim3_ext>) -> i32;

        pub fn GetPimMode(self: Pin<&mut dramsim3_ext>) -> bool;
        pub fn SetPimMode(self: Pin<&mut dramsim3_ext>, new_mode: bool);

        pub fn GetBytes(
            self: Pin<&mut dramsim3_ext>,
            start_addr: u64,
            data_index: &mut i64,
            start_byte: &mut u64,
        );
        pub fn GetSpatialGlobalAddr(
            self: Pin<&mut dramsim3_ext>,
            local_addr: &local_addr_bulk,
        ) -> u64;
        pub fn BankLocalToGlobalAddr(
            self: Pin<&mut dramsim3_ext>,
            local_addr: &local_addr_bulk,
        ) -> u64;
        pub fn ExactLocalToGlobalAddr(
            self: Pin<&mut dramsim3_ext>,
            local_addr: &local_addr_bulk,
        ) -> u64;
        pub fn GlobalToLocalAddr(self: Pin<&mut dramsim3_ext>, hex_addr: u64) -> local_addr_bulk;

        pub fn GetRanks(self: Pin<&mut dramsim3_ext>) -> u64;
        pub fn GetBanksPerBG(self: Pin<&mut dramsim3_ext>) -> u64;
        pub fn GetBankgroupsPerRank(self: Pin<&mut dramsim3_ext>) -> u64;
        pub fn GetChannels(self: Pin<&mut dramsim3_ext>) -> u64;

        pub fn WillAcceptTransaction(
            self: Pin<&mut dramsim3_ext>,
            hex_addr: u64,
            is_write: bool,
        ) -> bool;

        pub fn AddTransaction(
            self: Pin<&mut dramsim3_ext>,
            hex_addr: u64,
            is_write: bool,
            is_pim: bool,
        ) -> bool;

    }
}
