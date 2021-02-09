use std::borrow::Cow;

use crate::svd::{Access, Cluster, Register, RegisterCluster};
use inflections::Inflect;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{quote, ToTokens};

use anyhow::{anyhow, bail, Result};

pub const BITS_PER_BYTE: u32 = 8;

/// List of chars that some vendors use in their peripheral/field names but
/// that are not valid in Rust ident
const BLACKLIST_CHARS: &[char] = &['(', ')', '[', ']', '/', ' ', '-'];

#[derive(Clone, Copy, PartialEq)]
pub enum Target {
    CortexM,
    Msp430,
    RISCV,
    XtensaLX,
    Mips,
    None,
}

impl Target {
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "cortex-m" => Target::CortexM,
            "msp430" => Target::Msp430,
            "riscv" => Target::RISCV,
            "xtensa-lx" => Target::XtensaLX,
            "mips" => Target::Mips,
            "none" => Target::None,
            _ => bail!("unknown target {}", s),
        })
    }
}

pub trait ToSanitizedPascalCase {
    fn to_sanitized_pascal_case(&self) -> Cow<str>;
}

pub trait ToSanitizedUpperCase {
    fn to_sanitized_upper_case(&self) -> Cow<str>;
}

pub trait ToSanitizedSnakeCase {
    fn to_sanitized_snake_case(&self) -> Cow<str>;
}

impl ToSanitizedSnakeCase for str {
    fn to_sanitized_snake_case(&self) -> Cow<str> {
        macro_rules! keywords {
            ($s:expr, $($kw:ident),+,) => {
                Cow::from(match &$s.to_lowercase()[..] {
                    $(stringify!($kw) => concat!(stringify!($kw), "_")),+,
                    _ => return Cow::from($s.to_snake_case())
                })
            }
        }

        let s = self.replace(BLACKLIST_CHARS, "");

        match s.chars().next().unwrap_or('\0') {
            '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                Cow::from(format!("_{}", s.to_snake_case()))
            }
            _ => {
                keywords! {
                    s,
                    abstract,
                    alignof,
                    as,
                    async,
                    await,
                    become,
                    box,
                    break,
                    const,
                    continue,
                    crate,
                    do,
                    else,
                    enum,
                    extern,
                    false,
                    final,
                    fn,
                    for,
                    if,
                    impl,
                    in,
                    let,
                    loop,
                    macro,
                    match,
                    mod,
                    move,
                    mut,
                    offsetof,
                    override,
                    priv,
                    proc,
                    pub,
                    pure,
                    ref,
                    return,
                    self,
                    sizeof,
                    static,
                    struct,
                    super,
                    trait,
                    true,
                    try,
                    type,
                    typeof,
                    unsafe,
                    unsized,
                    use,
                    virtual,
                    where,
                    while,
                    yield,
                    set_bit,
                    clear_bit,
                    bit,
                    bits,
                }
            }
        }
    }
}

impl ToSanitizedUpperCase for str {
    fn to_sanitized_upper_case(&self) -> Cow<str> {
        let s = self.replace(BLACKLIST_CHARS, "");

        match s.chars().next().unwrap_or('\0') {
            '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                Cow::from(format!("_{}", s.to_upper_case()))
            }
            _ => Cow::from(s.to_upper_case()),
        }
    }
}

impl ToSanitizedPascalCase for str {
    fn to_sanitized_pascal_case(&self) -> Cow<str> {
        let s = self.replace(BLACKLIST_CHARS, "");

        match s.chars().next().unwrap_or('\0') {
            '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                Cow::from(format!("_{}", s.to_pascal_case()))
            }
            _ => Cow::from(s.to_pascal_case()),
        }
    }
}

pub fn respace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn escape_brackets(s: &str) -> String {
    s.split('[')
        .fold("".to_string(), |acc, x| {
            if acc == "" {
                x.to_string()
            } else if acc.ends_with('\\') {
                acc + "[" + x
            } else {
                acc + "\\[" + x
            }
        })
        .split(']')
        .fold("".to_string(), |acc, x| {
            if acc == "" {
                x.to_string()
            } else if acc.ends_with('\\') {
                acc + "]" + x
            } else {
                acc + "\\]" + x
            }
        })
}

pub fn name_of(register: &Register) -> Cow<str> {
    match register {
        Register::Single(info) => Cow::from(&info.name),
        Register::Array(info, _) => replace_suffix(&info.name, "").into(),
    }
}

pub fn replace_suffix(name: &str, suffix: &str) -> String {
    if name.contains("[%s]") {
        name.replace("[%s]", suffix)
    } else {
        name.replace("%s", suffix)
    }
}

pub fn access_of(register: &Register) -> Access {
    register.access.unwrap_or_else(|| {
        if let Some(fields) = &register.fields {
            if fields.iter().all(|f| f.access == Some(Access::ReadOnly)) {
                Access::ReadOnly
            } else if fields.iter().all(|f| f.access == Some(Access::WriteOnce)) {
                Access::WriteOnce
            } else if fields
                .iter()
                .all(|f| f.access == Some(Access::ReadWriteOnce))
            {
                Access::ReadWriteOnce
            } else if fields
                .iter()
                .all(|f| f.access == Some(Access::WriteOnly) || f.access == Some(Access::WriteOnce))
            {
                Access::WriteOnly
            } else {
                Access::ReadWrite
            }
        } else {
            Access::ReadWrite
        }
    })
}

/// Turns `n` into an unsuffixed separated hex token
pub fn hex(n: u64) -> TokenStream {
    let (h4, h3, h2, h1) = (
        (n >> 48) & 0xffff,
        (n >> 32) & 0xffff,
        (n >> 16) & 0xffff,
        n & 0xffff,
    );
    syn::parse_str::<syn::Lit>(
        &(if h4 != 0 {
            format!("0x{:04x}_{:04x}_{:04x}_{:04x}", h4, h3, h2, h1)
        } else if h3 != 0 {
            format!("0x{:04x}_{:04x}_{:04x}", h3, h2, h1)
        } else if h2 != 0 {
            format!("0x{:04x}_{:04x}", h2, h1)
        } else if h1 & 0xff00 != 0 {
            format!("0x{:04x}", h1)
        } else if h1 != 0 {
            format!("0x{:02x}", h1 & 0xff)
        } else {
            "0".to_string()
        }),
    )
    .unwrap()
    .into_token_stream()
}

/// Turns `n` into an unsuffixed token
pub fn unsuffixed(n: u64) -> TokenStream {
    Literal::u64_unsuffixed(n).into_token_stream()
}

pub fn unsuffixed_or_bool(n: u64, width: u32) -> TokenStream {
    if width == 1 {
        Ident::new(if n == 0 { "false" } else { "true" }, Span::call_site()).into_token_stream()
    } else {
        unsuffixed(n)
    }
}

pub trait U32Ext {
    fn to_ty(&self) -> Result<Ident>;
    fn to_ty_width(&self) -> Result<u32>;
}

impl U32Ext for u32 {
    fn to_ty(&self) -> Result<Ident> {
        Ok(Ident::new(
            match *self {
                1 => "bool",
                2..=8 => "u8",
                9..=16 => "u16",
                17..=32 => "u32",
                33..=64 => "u64",
                _ => {
                    return Err(anyhow!(
                        "can't convert {} bits into a Rust integral type",
                        *self
                    ))
                }
            },
            Span::call_site(),
        ))
    }

    fn to_ty_width(&self) -> Result<u32> {
        Ok(match *self {
            1 => 1,
            2..=8 => 8,
            9..=16 => 16,
            17..=32 => 32,
            33..=64 => 64,
            _ => {
                return Err(anyhow!(
                    "can't convert {} bits into a Rust integral type width",
                    *self
                ))
            }
        })
    }
}

/// Return the name of either register or cluster.
pub fn erc_name(erc: &RegisterCluster) -> &String {
    match erc {
        RegisterCluster::Register(r) => &r.name,
        RegisterCluster::Cluster(c) => &c.name,
    }
}

/// Return the name of either register or cluster from which this register or cluster is derived.
pub fn erc_derived_from(erc: &RegisterCluster) -> &Option<String> {
    match erc {
        RegisterCluster::Register(r) => &r.derived_from,
        RegisterCluster::Cluster(c) => &c.derived_from,
    }
}

/// Return only the clusters from the slice of either register or clusters.
pub fn only_clusters(ercs: &[RegisterCluster]) -> Vec<&Cluster> {
    let clusters: Vec<&Cluster> = ercs
        .iter()
        .filter_map(|x| match x {
            RegisterCluster::Cluster(x) => Some(x),
            _ => None,
        })
        .collect();
    clusters
}

/// Return only the registers the given slice of either register or clusters.
pub fn only_registers(ercs: &[RegisterCluster]) -> Vec<&Register> {
    let registers: Vec<&Register> = ercs
        .iter()
        .filter_map(|x| match x {
            RegisterCluster::Register(x) => Some(x),
            _ => None,
        })
        .collect();
    registers
}

pub fn build_rs() -> TokenStream {
    quote! {
        use std::env;
        use std::fs::File;
        use std::io::Write;
        use std::path::PathBuf;

        fn main() {
            if env::var_os("CARGO_FEATURE_RT").is_some() {
                // Put the linker script somewhere the linker can find it
                let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
                File::create(out.join("device.x"))
                    .unwrap()
                    .write_all(include_bytes!("device.x"))
                    .unwrap();
                println!("cargo:rustc-link-search={}", out.display());

                println!("cargo:rerun-if-changed=device.x");
            }

            println!("cargo:rerun-if-changed=build.rs");
        }
    }
}
