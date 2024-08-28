use solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPluginError;


/// Convenience helper which constructs a `GeyserPluginError::Custom` error.
pub fn custom_err<T>(err: T) -> GeyserPluginError
where
    T: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    GeyserPluginError::Custom(err.into())
}


/// Displays data buffer.
///
/// Displays the first 64 bytes of the slice in hex followed by its length.
/// Truncated data is indicated with an ellipsis.
pub struct DataDisplay<'a>(pub &'a [u8]);

impl<'a> core::fmt::Display for DataDisplay<'a> {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        let (data, suff) = if self.0.len() <= 64 {
            (self.0, "")
        } else {
            (&self.0[..64], "…")
        };
        write!(fmtr, "0x{}{suff} ({} bytes)", hex::display(&data), self.0.len())
    }
}
