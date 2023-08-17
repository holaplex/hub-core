/// Trait containing functions to validate Blockchain address
pub trait ValidateAddress {
    /// Checks if it is a valid Blockchain Address
    #[cfg(feature = "solana")]
    fn is_blockchain_address(&self) -> bool;

    /// Checks if it is an EVM address
    fn is_evm_address(&self) -> bool;

    /// Checks if it is a Solana address
    #[cfg(feature = "solana")]
    fn is_solana_address(&self) -> bool;
}

impl<S> ValidateAddress for S
where
    S: AsRef<str>,
{
    #[cfg(feature = "solana")]
    fn is_blockchain_address(&self) -> bool {
        self.is_evm_address() || self.is_solana_address()
    }

    fn is_evm_address(&self) -> bool {
        let address = self.as_ref();
        // EVM Address must start with 0x and have 42 characters
        if !address.starts_with("0x") || address.len() != 42 {
            return false;
        }

        // Check that the address contains only hexadecimal characters
        address[2..].chars().all(|c| c.is_ascii_hexdigit())
    }

    #[cfg(feature = "solana")]
    fn is_solana_address(&self) -> bool {
        use std::str::FromStr;

        solana_sdk::pubkey::Pubkey::from_str(self.as_ref()).is_ok()
    }
}
