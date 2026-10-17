use pulldown_cmark::Options;

/// Opções do `pulldown-cmark` usadas em todo o programa (renderização e extração veem a mesma estrutura).
pub fn parse_options() -> Options {
    // `ENABLE_MATH` fica de fora de propósito: textos de LLM com `$5 e $10` seriam lidos como fórmula.
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}

