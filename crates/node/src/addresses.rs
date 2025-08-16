pub struct SepoliaAddresses {
    pub sequencer_inbox: &'static str,
    pub delayed_inbox: &'static str,
    pub bridge: &'static str,
    pub outbox: &'static str,
}

pub const ARB_SEPOLIA: SepoliaAddresses = SepoliaAddresses {
    sequencer_inbox: "0x6c97864C01d5903BC8C9C3688B1560a42F1ebE0D",
    delayed_inbox: "0xaAe29B0366299461418F5324a79Afc425BE5ae21",
    bridge: "0x38f918D0E7b4E9b3814dFf41b1e1B3A0D89333a9",
    outbox: "0x65f07C7D22A4166c5E8Fc5F41Af23a2C8a2B78fF",
};
