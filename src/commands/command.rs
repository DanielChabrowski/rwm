pub trait Command {
    fn execute(&self) -> anyhow::Result<()>;
}

pub type BoxCommand = Box<dyn Command>;
