use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use syn::{
    Attribute, Fields, GenericArgument, Item, LitStr, PathArguments, Type, TypePath, Visibility,
};

const CONTRACT_FILES: &[&str] = &["api.rs", "features.rs", "jobs.rs", "media.rs"];

/// The compatibility DTOs the React app actually exchanges for renaming,
/// track properties, mux/remux, and the library audit.
///
/// These live in the runtime rather than the contracts crate, and were left out
/// of generation for that reason -- so the drift check passed while the two
/// hand-written copies of `RenameSearchResult` disagreed about whether an id is
/// a number. They are generated on the same terms as everything else now.
const COMPATIBILITY_FILE: &str = "crates/mkvo-runtime/src/compat.rs";
const DOMAIN_ENUM_FILES: &[(&str, &str)] = &[
    ("MediaServerKind", "settings.rs"),
    ("MetadataProvider", "media.rs"),
    ("MediaStatus", "media.rs"),
    ("RemuxMode", "remux.rs"),
    ("TrackKind", "media.rs"),
];

const GENERATION_GAPS: &[(&str, &str)] = &[
    (
        "ApiEnvelope",
        "Generic host envelope; current Tauri and HTTP compatibility adapters return the enclosed DTO directly.",
    ),
    (
        "ApiResult",
        "Rust control-flow alias (Result), not a JSON boundary DTO.",
    ),
    (
        "LibraryAuditDomainResponse",
        "Domain-native audit graph; React consumes LibraryAuditResponse compatibility rows.",
    ),
    (
        "PropertyEditPlanResponse",
        "Domain-native execution plan; React consumes preview DTOs plus plan identifiers.",
    ),
    (
        "RemuxPlanResponse",
        "Domain-native execution plan; React consumes preview DTOs plus plan identifiers.",
    ),
    (
        "RenamePlanResponse",
        "Domain-native execution plan; React consumes preview DTOs plus plan identifiers.",
    ),
    (
        "SaveSettingsRequest",
        "Domain-native AppSettings request; the compatibility UI uses WebSettingsRequest.",
    ),
    (
        "SettingsResponse",
        "Domain-native AppSettings response; the compatibility UI uses WebSettings.",
    ),
];

const STRING_WIRE_TYPES: &[&str] = &[
    "CorrelationId",
    "IdempotencyKey",
    "JobId",
    "MediaServerId",
    "PlanId",
    "RenameBatchId",
    "Uuid",
    "Url",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Origin {
    Contract,
    Compatibility,
    DomainDependency,
}

#[derive(Clone, Debug, Default)]
struct SerdeOptions {
    rename: Option<String>,
    rename_all: Option<String>,
    rename_all_fields: Option<String>,
    tag: Option<String>,
    content: Option<String>,
    default: bool,
    flatten: bool,
    skip: bool,
    skip_serializing: bool,
    skip_serializing_if: bool,
    transparent: bool,
    untagged: bool,
}

#[derive(Clone, Debug)]
struct FieldDefinition {
    name: String,
    ty: Type,
    serde: SerdeOptions,
}

#[derive(Clone, Debug)]
enum VariantFields {
    Unit,
    Named(Vec<FieldDefinition>),
    Unnamed(Vec<Type>),
}

#[derive(Clone, Debug)]
struct VariantDefinition {
    name: String,
    serde: SerdeOptions,
    fields: VariantFields,
}

#[derive(Clone, Debug)]
enum DefinitionKind {
    Struct(Vec<FieldDefinition>),
    Enum(Vec<VariantDefinition>),
    Alias(Type),
}

#[derive(Clone, Debug)]
struct Definition {
    name: String,
    origin: Origin,
    serde: SerdeOptions,
    generic: bool,
    kind: DefinitionKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TsType {
    String,
    Number { integer: bool, unsigned: bool },
    Boolean,
    Null,
    Json,
    Array(Box<Self>),
    Record(Box<Self>),
    Tuple(Vec<Self>),
    Ref(String),
    Union(Vec<Self>),
}

#[derive(Debug)]
struct GeneratedArtifacts {
    typescript: String,
    readme: String,
}

fn main() -> Result<()> {
    let mode = env::args().nth(1).unwrap_or_else(|| "write".to_owned());
    let root = workspace_root();
    let artifacts = generate(&root)?;
    let generated_dir = root.join("web/src/generated");
    let typescript_path = generated_dir.join("contracts.ts");
    let readme_path = generated_dir.join("README.md");

    match mode.as_str() {
        "write" | "--write" => {
            fs::create_dir_all(&generated_dir)
                .with_context(|| format!("creating {}", generated_dir.display()))?;
            fs::write(&typescript_path, artifacts.typescript)
                .with_context(|| format!("writing {}", typescript_path.display()))?;
            fs::write(&readme_path, artifacts.readme)
                .with_context(|| format!("writing {}", readme_path.display()))?;
            println!("generated {}", typescript_path.display());
            println!("generated {}", readme_path.display());
        }
        "check" | "--check" => {
            check_file(&typescript_path, &artifacts.typescript)?;
            check_file(&readme_path, &artifacts.readme)?;
            println!("generated contracts are current");
        }
        "print" | "--print" => print!("{}", artifacts.typescript),
        _ => bail!("unknown mode {mode:?}; expected write, --check, or --print"),
    }
    Ok(())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("generator crate must be two levels below the workspace root")
        .to_path_buf()
}

fn check_file(path: &Path, expected: &str) -> Result<()> {
    let actual = fs::read_to_string(path)
        .with_context(|| format!("{} is missing; run the contract generator", path.display()))?;
    if actual != expected {
        bail!(
            "{} has drifted from the Rust contracts; run `cargo run -p mkvo-contract-gen -- write`",
            path.display()
        );
    }
    Ok(())
}

fn generate(root: &Path) -> Result<GeneratedArtifacts> {
    let mut definitions = BTreeMap::new();
    let contract_dir = root.join("crates/mkvo-contracts/src");
    for file_name in CONTRACT_FILES {
        load_definitions(
            &contract_dir.join(file_name),
            Origin::Contract,
            None,
            &mut definitions,
        )?;
    }

    load_definitions(
        &root.join(COMPATIBILITY_FILE),
        Origin::Compatibility,
        None,
        &mut definitions,
    )?;

    let domain_dir = root.join("crates/mkvo-domain/src");
    for (type_name, file_name) in DOMAIN_ENUM_FILES {
        load_definitions(
            &domain_dir.join(file_name),
            Origin::DomainDependency,
            Some(type_name),
            &mut definitions,
        )?;
    }

    validate_coverage(&definitions)?;
    validate_references(&definitions)?;

    Ok(GeneratedArtifacts {
        typescript: render_typescript(&definitions)?,
        readme: render_readme(&definitions),
    })
}

fn load_definitions(
    path: &Path,
    origin: Origin,
    only_name: Option<&str>,
    definitions: &mut BTreeMap<String, Definition>,
) -> Result<()> {
    let source = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let syntax = syn::parse_file(&source).with_context(|| format!("parsing {}", path.display()))?;
    for item in syntax.items {
        let Some(definition) = parse_definition(item, origin)? else {
            continue;
        };
        if only_name.is_some_and(|name| name != definition.name) {
            continue;
        }
        // Fourteen names exist in both the contracts crate and the
        // compatibility layer with different shapes. The hosts serialize the
        // compatibility one, so that is the wire truth and it wins; generating
        // the other would describe a payload nothing sends.
        let replacing = definitions.insert(definition.name.clone(), definition);
        if let Some(previous) = replacing
            && !(previous.origin == Origin::Contract && origin == Origin::Compatibility)
        {
            bail!(
                "duplicate contract type {} while parsing {}",
                previous.name,
                path.display()
            );
        }
    }
    if let Some(name) = only_name
        && !definitions.contains_key(name)
    {
        bail!(
            "could not find required domain enum {name} in {}",
            path.display()
        );
    }
    Ok(())
}

fn parse_definition(item: Item, origin: Origin) -> Result<Option<Definition>> {
    match item {
        Item::Struct(item) if is_public(&item.vis) => {
            let name = item.ident.to_string();
            let serde = parse_serde_options(&item.attrs)?;
            let generic = !item.generics.params.is_empty();
            let fields = parse_named_fields(item.fields, &name)?;
            Ok(Some(Definition {
                name,
                origin,
                serde,
                generic,
                kind: DefinitionKind::Struct(fields),
            }))
        }
        Item::Enum(item) if is_public(&item.vis) => {
            let name = item.ident.to_string();
            let serde = parse_serde_options(&item.attrs)?;
            let generic = !item.generics.params.is_empty();
            let mut variants = Vec::new();
            for variant in item.variants {
                let fields = match variant.fields {
                    Fields::Unit => VariantFields::Unit,
                    Fields::Named(fields) => VariantFields::Named(
                        fields
                            .named
                            .into_iter()
                            .map(parse_field)
                            .collect::<Result<Vec<_>>>()?,
                    ),
                    Fields::Unnamed(fields) => VariantFields::Unnamed(
                        fields.unnamed.into_iter().map(|field| field.ty).collect(),
                    ),
                };
                variants.push(VariantDefinition {
                    name: variant.ident.to_string(),
                    serde: parse_serde_options(&variant.attrs)?,
                    fields,
                });
            }
            Ok(Some(Definition {
                name,
                origin,
                serde,
                generic,
                kind: DefinitionKind::Enum(variants),
            }))
        }
        Item::Type(item) if is_public(&item.vis) => Ok(Some(Definition {
            name: item.ident.to_string(),
            origin,
            serde: SerdeOptions::default(),
            generic: !item.generics.params.is_empty(),
            kind: DefinitionKind::Alias(*item.ty),
        })),
        _ => Ok(None),
    }
}

fn is_public(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Public(_))
}

fn parse_named_fields(fields: Fields, type_name: &str) -> Result<Vec<FieldDefinition>> {
    match fields {
        Fields::Named(fields) => fields
            .named
            .into_iter()
            .map(parse_field)
            .collect::<Result<Vec<_>>>(),
        Fields::Unit => Ok(Vec::new()),
        Fields::Unnamed(_) => bail!("tuple struct {type_name} is not a supported boundary DTO"),
    }
}

fn parse_field(field: syn::Field) -> Result<FieldDefinition> {
    let name = field
        .ident
        .as_ref()
        .ok_or_else(|| anyhow!("boundary DTO field must be named"))?
        .to_string();
    Ok(FieldDefinition {
        name,
        ty: field.ty,
        serde: parse_serde_options(&field.attrs)?,
    })
}

fn parse_serde_options(attributes: &[Attribute]) -> Result<SerdeOptions> {
    let mut options = SerdeOptions::default();
    for attribute in attributes {
        if !attribute.path().is_ident("serde") {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                options.rename = Some(meta.value()?.parse::<LitStr>()?.value());
            } else if meta.path.is_ident("rename_all") {
                options.rename_all = Some(meta.value()?.parse::<LitStr>()?.value());
            } else if meta.path.is_ident("rename_all_fields") {
                options.rename_all_fields = Some(meta.value()?.parse::<LitStr>()?.value());
            } else if meta.path.is_ident("tag") {
                options.tag = Some(meta.value()?.parse::<LitStr>()?.value());
            } else if meta.path.is_ident("content") {
                options.content = Some(meta.value()?.parse::<LitStr>()?.value());
            } else if meta.path.is_ident("default") {
                options.default = true;
                if meta.input.peek(syn::Token![=]) {
                    let _ = meta.value()?.parse::<LitStr>()?;
                }
            } else if meta.path.is_ident("flatten") {
                options.flatten = true;
            } else if meta.path.is_ident("skip") {
                options.skip = true;
            } else if meta.path.is_ident("skip_serializing") {
                options.skip_serializing = true;
            } else if meta.path.is_ident("skip_serializing_if") {
                options.skip_serializing_if = true;
                let _ = meta.value()?.parse::<LitStr>()?;
            } else if meta.path.is_ident("transparent") {
                options.transparent = true;
            } else if meta.path.is_ident("untagged") {
                options.untagged = true;
            }
            Ok(())
        })?;
    }
    Ok(options)
}

fn validate_coverage(definitions: &BTreeMap<String, Definition>) -> Result<()> {
    for definition in definitions.values() {
        if definition.origin != Origin::Contract {
            continue;
        }
        let gap = gap_reason(&definition.name);
        if definition.generic && gap.is_none() {
            bail!(
                "generic public contract {} must be supported or added to GENERATION_GAPS with a reason",
                definition.name
            );
        }
    }
    for (name, _) in GENERATION_GAPS {
        if !definitions.contains_key(*name) {
            bail!("stale GENERATION_GAPS entry for missing Rust type {name}");
        }
    }
    Ok(())
}

fn validate_references(definitions: &BTreeMap<String, Definition>) -> Result<()> {
    let generated_names: BTreeSet<_> = generated_definitions(definitions)
        .map(|definition| definition.name.as_str())
        .collect();
    for definition in generated_definitions(definitions) {
        let mut references = BTreeSet::new();
        collect_definition_references(definition, &mut references)?;
        for reference in references {
            if reference == "JsonValue" || generated_names.contains(reference.as_str()) {
                continue;
            }
            bail!(
                "generated contract {} references unsupported type {reference}; add a wire mapping, generate the dependency, or explicitly exclude the contract",
                definition.name
            );
        }
    }
    Ok(())
}

fn generated_definitions(
    definitions: &BTreeMap<String, Definition>,
) -> impl Iterator<Item = &Definition> {
    definitions
        .values()
        .filter(|definition| gap_reason(&definition.name).is_none())
}

fn gap_reason(name: &str) -> Option<&'static str> {
    GENERATION_GAPS
        .iter()
        .find_map(|(gap_name, reason)| (*gap_name == name).then_some(*reason))
}

fn collect_definition_references(
    definition: &Definition,
    references: &mut BTreeSet<String>,
) -> Result<()> {
    match &definition.kind {
        DefinitionKind::Struct(fields) => {
            for field in fields {
                collect_type_references(&map_type(&field.ty)?, references);
            }
        }
        DefinitionKind::Enum(variants) => {
            for variant in variants {
                match &variant.fields {
                    VariantFields::Unit => {}
                    VariantFields::Named(fields) => {
                        for field in fields {
                            collect_type_references(&map_type(&field.ty)?, references);
                        }
                    }
                    VariantFields::Unnamed(fields) => {
                        for field in fields {
                            collect_type_references(&map_type(field)?, references);
                        }
                    }
                }
            }
        }
        DefinitionKind::Alias(ty) => collect_type_references(&map_type(ty)?, references),
    }
    Ok(())
}

fn collect_type_references(ty: &TsType, references: &mut BTreeSet<String>) {
    match ty {
        TsType::Ref(name) => {
            references.insert(name.clone());
        }
        TsType::Array(inner) | TsType::Record(inner) => {
            collect_type_references(inner, references);
        }
        TsType::Tuple(items) | TsType::Union(items) => {
            for item in items {
                collect_type_references(item, references);
            }
        }
        TsType::String | TsType::Number { .. } | TsType::Boolean | TsType::Null | TsType::Json => {}
    }
}

fn map_type(ty: &Type) -> Result<TsType> {
    match ty {
        Type::Path(path) => map_path_type(path),
        Type::Reference(reference) => map_type(&reference.elem),
        Type::Tuple(tuple) if tuple.elems.is_empty() => Ok(TsType::Null),
        Type::Tuple(tuple) => tuple
            .elems
            .iter()
            .map(map_type)
            .collect::<Result<Vec<_>>>()
            .map(TsType::Tuple),
        Type::Paren(paren) => map_type(&paren.elem),
        Type::Group(group) => map_type(&group.elem),
        unsupported => bail!("unsupported Rust boundary type: {unsupported:?}"),
    }
}

fn map_path_type(path: &TypePath) -> Result<TsType> {
    let segment = path
        .path
        .segments
        .last()
        .ok_or_else(|| anyhow!("empty Rust type path"))?;
    let name = segment.ident.to_string();
    let arguments = type_arguments(&segment.arguments)?;
    match name.as_str() {
        "String" | "str" | "Path" | "PathBuf" if arguments.is_empty() => Ok(TsType::String),
        name if STRING_WIRE_TYPES.contains(&name) && arguments.is_empty() => Ok(TsType::String),
        "bool" if arguments.is_empty() => Ok(TsType::Boolean),
        "f32" | "f64" if arguments.is_empty() => Ok(TsType::Number {
            integer: false,
            unsigned: false,
        }),
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" if arguments.is_empty() => {
            Ok(TsType::Number {
                integer: true,
                unsigned: false,
            })
        }
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" if arguments.is_empty() => {
            Ok(TsType::Number {
                integer: true,
                unsigned: true,
            })
        }
        "DateTime" | "NaiveDate" | "NaiveDateTime" if arguments.len() <= 1 => Ok(TsType::String),
        "Value" if arguments.is_empty() => Ok(TsType::Json),
        "Option" if arguments.len() == 1 => Ok(union(vec![map_type(arguments[0])?, TsType::Null])),
        "Vec" | "VecDeque" | "BTreeSet" | "HashSet" if arguments.len() == 1 => {
            Ok(TsType::Array(Box::new(map_type(arguments[0])?)))
        }
        "BTreeMap" | "HashMap" if arguments.len() == 2 => {
            Ok(TsType::Record(Box::new(map_type(arguments[1])?)))
        }
        "Box" | "Arc" | "Rc" | "Cow" if arguments.len() == 1 => map_type(arguments[0]),
        "Result" if arguments.len() == 2 => Ok(union(vec![
            map_type(arguments[0])?,
            map_type(arguments[1])?,
        ])),
        _ if arguments.is_empty() => Ok(TsType::Ref(name)),
        _ => bail!("unsupported generic Rust boundary type {name}"),
    }
}

fn type_arguments(arguments: &PathArguments) -> Result<Vec<&Type>> {
    match arguments {
        PathArguments::None => Ok(Vec::new()),
        PathArguments::AngleBracketed(arguments) => Ok(arguments
            .args
            .iter()
            .filter_map(|argument| match argument {
                GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
            .collect()),
        PathArguments::Parenthesized(_) => bail!("function types are not boundary DTO fields"),
    }
}

fn union(items: Vec<TsType>) -> TsType {
    let mut flattened = Vec::new();
    for item in items {
        match item {
            TsType::Union(nested) => flattened.extend(nested),
            other => flattened.push(other),
        }
    }
    let mut unique = Vec::new();
    for item in flattened {
        if !unique.contains(&item) {
            unique.push(item);
        }
    }
    if unique.len() == 1 {
        unique.pop().expect("one union item")
    } else {
        TsType::Union(unique)
    }
}

fn render_typescript(definitions: &BTreeMap<String, Definition>) -> Result<String> {
    let generated: Vec<_> = generated_definitions(definitions).collect();
    let mut output = String::from(
        "// @generated by `cargo run -p mkvo-contract-gen -- write`; DO NOT EDIT.\n\
         // Rust serde DTOs are authoritative. `--check` fails when this file drifts.\n\
         // Response fields are optional only when serde may omit them while serializing.\n\
         // Option<T> fields on *Request DTOs are also optional because serde accepts missing options.\n\n\
         export type JsonValue =\n\
           | null\n\
           | boolean\n\
           | number\n\
           | string\n\
           | JsonValue[]\n\
           | { [key: string]: JsonValue };\n\n",
    );

    for definition in &generated {
        output.push_str(&render_declaration(definition)?);
        output.push('\n');
    }

    output.push_str("export interface ContractTypes {\n");
    for definition in &generated {
        output.push_str(&format!("  {}: {};\n", definition.name, definition.name));
    }
    output.push_str("}\n\nexport type ContractName = keyof ContractTypes;\n\n");

    output.push_str(
        "type ContractSchema =\n\
           | { readonly kind: \"string\" }\n\
           | { readonly kind: \"number\"; readonly integer: boolean; readonly unsigned: boolean }\n\
           | { readonly kind: \"boolean\" }\n\
           | { readonly kind: \"null\" }\n\
           | { readonly kind: \"unknown\" }\n\
           | { readonly kind: \"literal\"; readonly value: string }\n\
           | { readonly kind: \"array\"; readonly item: ContractSchema }\n\
           | { readonly kind: \"record\"; readonly value: ContractSchema }\n\
           | { readonly kind: \"tuple\"; readonly items: readonly ContractSchema[] }\n\
           | { readonly kind: \"ref\"; readonly name: string }\n\
           | { readonly kind: \"union\"; readonly variants: readonly ContractSchema[] }\n\
           | {\n\
               readonly kind: \"object\";\n\
               readonly fields: Readonly<Record<string, { readonly optional: boolean; readonly schema: ContractSchema }>>;\n\
               readonly flatten: readonly ContractSchema[];\n\
             };\n\n",
    );

    output.push_str("export const contractSchemas: Record<ContractName, ContractSchema> = {\n");
    for definition in &generated {
        output.push_str(&format!(
            "  {}: {},\n",
            definition.name,
            render_definition_schema(definition)?
        ));
    }
    output.push_str("};\n\n");

    output.push_str("export const CONTRACT_GENERATION_GAPS = {\n");
    for (name, reason) in GENERATION_GAPS {
        output.push_str(&format!("  {}: {},\n", ts_string(name), ts_string(reason)));
    }
    output.push_str("} as const;\n\n");

    output.push_str(VALIDATOR_RUNTIME);
    Ok(output)
}

fn render_declaration(definition: &Definition) -> Result<String> {
    match &definition.kind {
        DefinitionKind::Struct(fields) => render_struct_declaration(definition, fields),
        DefinitionKind::Enum(variants) => render_enum_declaration(definition, variants),
        DefinitionKind::Alias(ty) => Ok(format!(
            "export type {} = {};\n",
            definition.name,
            render_ts_type(&map_type(ty)?)
        )),
    }
}

fn render_struct_declaration(
    definition: &Definition,
    fields: &[FieldDefinition],
) -> Result<String> {
    if definition.serde.transparent {
        let field = fields
            .first()
            .ok_or_else(|| anyhow!("transparent DTO {} has no field", definition.name))?;
        return Ok(format!(
            "export type {} = {};\n",
            definition.name,
            render_ts_type(&map_type(&field.ty)?)
        ));
    }

    let flattened: Vec<_> = fields
        .iter()
        .filter(|field| field.serde.flatten && !field_is_skipped(field))
        .collect();
    let regular: Vec<_> = fields
        .iter()
        .filter(|field| !field.serde.flatten && !field_is_skipped(field))
        .collect();
    if flattened.is_empty() {
        let mut rendered = format!("export interface {} {{\n", definition.name);
        rendered.push_str(&render_fields(definition, &regular)?);
        rendered.push_str("}\n");
        return Ok(rendered);
    }

    let mut parts = flattened
        .iter()
        .map(|field| map_type(&field.ty).map(|ty| render_ts_type(&ty)))
        .collect::<Result<Vec<_>>>()?;
    let mut own_fields = String::from("{\n");
    own_fields.push_str(&render_fields(definition, &regular)?);
    own_fields.push('}');
    parts.push(own_fields);
    Ok(format!(
        "export type {} = {};\n",
        definition.name,
        parts.join(" & ")
    ))
}

fn render_fields(definition: &Definition, fields: &[&FieldDefinition]) -> Result<String> {
    let mut rendered = String::new();
    for field in fields {
        let wire_name = field_wire_name(field, definition.serde.rename_all.as_deref());
        let optional = field_is_optional(definition, field);
        rendered.push_str(&format!(
            "  {}{}: {};\n",
            ts_string(&wire_name),
            if optional { "?" } else { "" },
            render_ts_type(&map_type(&field.ty)?)
        ));
    }
    Ok(rendered)
}

fn render_enum_declaration(
    definition: &Definition,
    variants: &[VariantDefinition],
) -> Result<String> {
    if variants
        .iter()
        .all(|variant| matches!(variant.fields, VariantFields::Unit))
    {
        let values = variants
            .iter()
            .filter(|variant| !variant.serde.skip)
            .map(|variant| ts_string(&variant_wire_name(definition, variant)))
            .collect::<Vec<_>>();
        return Ok(format!(
            "export type {} = {};\n",
            definition.name,
            values.join(" | ")
        ));
    }

    let variants = variants
        .iter()
        .filter(|variant| !variant.serde.skip)
        .map(|variant| render_data_variant(definition, variant))
        .collect::<Result<Vec<_>>>()?;
    Ok(format!(
        "export type {} =\n  {};\n",
        definition.name,
        variants.join("\n  | ")
    ))
}

fn render_data_variant(definition: &Definition, variant: &VariantDefinition) -> Result<String> {
    let wire_name = variant_wire_name(definition, variant);
    if let Some(tag) = &definition.serde.tag {
        if definition.serde.content.is_some() {
            bail!(
                "adjacently tagged enum {} is not yet supported",
                definition.name
            );
        }
        return match &variant.fields {
            VariantFields::Unit => Ok(format!(
                "{{ {}: {} }}",
                ts_string(tag),
                ts_string(&wire_name)
            )),
            VariantFields::Named(fields) => {
                let mut rendered = format!("{{ {}: {};", ts_string(tag), ts_string(&wire_name));
                for field in fields.iter().filter(|field| !field_is_skipped(field)) {
                    let field_case = definition.serde.rename_all_fields.as_deref();
                    let field_name = field_wire_name(field, field_case);
                    rendered.push_str(&format!(
                        " {}{}: {};",
                        ts_string(&field_name),
                        if variant_field_is_optional(field) {
                            "?"
                        } else {
                            ""
                        },
                        render_ts_type(&map_type(&field.ty)?)
                    ));
                }
                rendered.push_str(" }");
                Ok(rendered)
            }
            VariantFields::Unnamed(fields) if fields.len() == 1 => Ok(format!(
                "{{ {}: {}; value: {} }}",
                ts_string(tag),
                ts_string(&wire_name),
                render_ts_type(&map_type(&fields[0])?)
            )),
            VariantFields::Unnamed(_) => bail!(
                "tuple variant {}::{} is not supported for an internally tagged enum",
                definition.name,
                variant.name
            ),
        };
    }

    if definition.serde.untagged {
        return match &variant.fields {
            VariantFields::Unit => Ok("null".to_owned()),
            VariantFields::Named(fields) => render_inline_object(definition, fields),
            VariantFields::Unnamed(fields) if fields.len() == 1 => {
                Ok(render_ts_type(&map_type(&fields[0])?))
            }
            VariantFields::Unnamed(fields) => fields
                .iter()
                .map(map_type)
                .collect::<Result<Vec<_>>>()
                .map(|items| render_ts_type(&TsType::Tuple(items))),
        };
    }

    match &variant.fields {
        VariantFields::Unit => Ok(ts_string(&wire_name)),
        VariantFields::Named(fields) => Ok(format!(
            "{{ {}: {} }}",
            ts_string(&wire_name),
            render_inline_object(definition, fields)?
        )),
        VariantFields::Unnamed(fields) if fields.len() == 1 => Ok(format!(
            "{{ {}: {} }}",
            ts_string(&wire_name),
            render_ts_type(&map_type(&fields[0])?)
        )),
        VariantFields::Unnamed(fields) => {
            let tuple = fields.iter().map(map_type).collect::<Result<Vec<_>>>()?;
            Ok(format!(
                "{{ {}: {} }}",
                ts_string(&wire_name),
                render_ts_type(&TsType::Tuple(tuple))
            ))
        }
    }
}

fn render_inline_object(definition: &Definition, fields: &[FieldDefinition]) -> Result<String> {
    let mut rendered = String::from("{");
    for field in fields.iter().filter(|field| !field_is_skipped(field)) {
        let field_name = field_wire_name(field, definition.serde.rename_all_fields.as_deref());
        rendered.push_str(&format!(
            " {}{}: {};",
            ts_string(&field_name),
            if variant_field_is_optional(field) {
                "?"
            } else {
                ""
            },
            render_ts_type(&map_type(&field.ty)?)
        ));
    }
    rendered.push_str(" }");
    Ok(rendered)
}

fn render_ts_type(ty: &TsType) -> String {
    match ty {
        TsType::String => "string".to_owned(),
        TsType::Number { .. } => "number".to_owned(),
        TsType::Boolean => "boolean".to_owned(),
        TsType::Null => "null".to_owned(),
        TsType::Json => "JsonValue".to_owned(),
        TsType::Array(inner) => format!("Array<{}>", render_ts_type(inner)),
        TsType::Record(inner) => format!("Record<string, {}>", render_ts_type(inner)),
        TsType::Tuple(items) => format!(
            "[{}]",
            items
                .iter()
                .map(render_ts_type)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TsType::Ref(name) => name.clone(),
        TsType::Union(items) => items
            .iter()
            .map(render_ts_type)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

fn render_definition_schema(definition: &Definition) -> Result<String> {
    match &definition.kind {
        DefinitionKind::Struct(fields) => render_struct_schema(definition, fields),
        DefinitionKind::Enum(variants) => render_enum_schema(definition, variants),
        DefinitionKind::Alias(ty) => render_type_schema(&map_type(ty)?),
    }
}

fn render_struct_schema(definition: &Definition, fields: &[FieldDefinition]) -> Result<String> {
    if definition.serde.transparent {
        let field = fields
            .first()
            .ok_or_else(|| anyhow!("transparent DTO {} has no field", definition.name))?;
        return render_type_schema(&map_type(&field.ty)?);
    }
    render_object_schema(fields, definition.serde.rename_all.as_deref(), |field| {
        field_is_optional(definition, field)
    })
}

fn render_object_schema(
    fields: &[FieldDefinition],
    rename_all: Option<&str>,
    optional: impl Fn(&FieldDefinition) -> bool,
) -> Result<String> {
    let mut regular = Vec::new();
    let mut flattened = Vec::new();
    for field in fields.iter().filter(|field| !field_is_skipped(field)) {
        let schema = render_type_schema(&map_type(&field.ty)?)?;
        if field.serde.flatten {
            flattened.push(schema);
        } else {
            regular.push(format!(
                "{}: {{ optional: {}, schema: {} }}",
                ts_string(&field_wire_name(field, rename_all)),
                optional(field),
                schema
            ));
        }
    }
    Ok(format!(
        "{{ kind: \"object\", fields: {{ {} }}, flatten: [{}] }}",
        regular.join(", "),
        flattened.join(", ")
    ))
}

fn render_enum_schema(definition: &Definition, variants: &[VariantDefinition]) -> Result<String> {
    let mut schemas = Vec::new();
    for variant in variants.iter().filter(|variant| !variant.serde.skip) {
        let wire_name = variant_wire_name(definition, variant);
        if matches!(variant.fields, VariantFields::Unit) && definition.serde.tag.is_none() {
            schemas.push(format!(
                "{{ kind: \"literal\", value: {} }}",
                ts_string(&wire_name)
            ));
            continue;
        }
        if let Some(tag) = &definition.serde.tag {
            if definition.serde.content.is_some() {
                bail!(
                    "adjacently tagged enum {} is not yet supported",
                    definition.name
                );
            }
            let mut fields = vec![format!(
                "{}: {{ optional: false, schema: {{ kind: \"literal\", value: {} }} }}",
                ts_string(tag),
                ts_string(&wire_name)
            )];
            match &variant.fields {
                VariantFields::Unit => {}
                VariantFields::Named(named) => {
                    for field in named.iter().filter(|field| !field_is_skipped(field)) {
                        fields.push(format!(
                            "{}: {{ optional: {}, schema: {} }}",
                            ts_string(&field_wire_name(
                                field,
                                definition.serde.rename_all_fields.as_deref()
                            )),
                            variant_field_is_optional(field),
                            render_type_schema(&map_type(&field.ty)?)?
                        ));
                    }
                }
                VariantFields::Unnamed(_) => bail!(
                    "unnamed internally tagged variant {}::{} is unsupported",
                    definition.name,
                    variant.name
                ),
            }
            schemas.push(format!(
                "{{ kind: \"object\", fields: {{ {} }}, flatten: [] }}",
                fields.join(", ")
            ));
            continue;
        }
        bail!(
            "data enum {} requires explicit tagged or untagged schema support",
            definition.name
        );
    }
    Ok(if schemas.len() == 1 {
        schemas.pop().expect("one enum schema")
    } else {
        format!("{{ kind: \"union\", variants: [{}] }}", schemas.join(", "))
    })
}

fn render_type_schema(ty: &TsType) -> Result<String> {
    Ok(match ty {
        TsType::String => "{ kind: \"string\" }".to_owned(),
        TsType::Number { integer, unsigned } => {
            format!("{{ kind: \"number\", integer: {integer}, unsigned: {unsigned} }}")
        }
        TsType::Boolean => "{ kind: \"boolean\" }".to_owned(),
        TsType::Null => "{ kind: \"null\" }".to_owned(),
        TsType::Json => "{ kind: \"unknown\" }".to_owned(),
        TsType::Array(inner) => format!(
            "{{ kind: \"array\", item: {} }}",
            render_type_schema(inner)?
        ),
        TsType::Record(inner) => format!(
            "{{ kind: \"record\", value: {} }}",
            render_type_schema(inner)?
        ),
        TsType::Tuple(items) => format!(
            "{{ kind: \"tuple\", items: [{}] }}",
            items
                .iter()
                .map(render_type_schema)
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        ),
        TsType::Ref(name) => format!("{{ kind: \"ref\", name: {} }}", ts_string(name)),
        TsType::Union(items) => format!(
            "{{ kind: \"union\", variants: [{}] }}",
            items
                .iter()
                .map(render_type_schema)
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        ),
    })
}

fn field_is_skipped(field: &FieldDefinition) -> bool {
    field.serde.skip || field.serde.skip_serializing
}

fn field_is_optional(definition: &Definition, field: &FieldDefinition) -> bool {
    field.serde.skip_serializing_if
        || (definition.name.ends_with("Request") && is_option(&field.ty))
}

fn variant_field_is_optional(field: &FieldDefinition) -> bool {
    field.serde.skip_serializing_if
}

fn is_option(ty: &Type) -> bool {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Option"),
        Type::Paren(paren) => is_option(&paren.elem),
        _ => false,
    }
}

fn field_wire_name(field: &FieldDefinition, rename_all: Option<&str>) -> String {
    field
        .serde
        .rename
        .clone()
        .unwrap_or_else(|| apply_case(&field.name, rename_all))
}

fn variant_wire_name(definition: &Definition, variant: &VariantDefinition) -> String {
    variant
        .serde
        .rename
        .clone()
        .unwrap_or_else(|| apply_case(&variant.name, definition.serde.rename_all.as_deref()))
}

fn apply_case(value: &str, case: Option<&str>) -> String {
    let Some(case) = case else {
        return value.to_owned();
    };
    let words = split_words(value);
    match case {
        "camelCase" => words
            .iter()
            .enumerate()
            .map(|(index, word)| {
                if index == 0 {
                    word.to_ascii_lowercase()
                } else {
                    capitalize(word)
                }
            })
            .collect(),
        "PascalCase" => words.iter().map(|word| capitalize(word)).collect(),
        "snake_case" => words.join("_").to_ascii_lowercase(),
        "SCREAMING_SNAKE_CASE" => words.join("_").to_ascii_uppercase(),
        "kebab-case" => words.join("-").to_ascii_lowercase(),
        "SCREAMING-KEBAB-CASE" => words.join("-").to_ascii_uppercase(),
        "lowercase" => words.concat().to_ascii_lowercase(),
        "UPPERCASE" => words.concat().to_ascii_uppercase(),
        _ => value.to_owned(),
    }
}

fn split_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let chars: Vec<_> = value.chars().collect();
    for (index, character) in chars.iter().copied().enumerate() {
        if character == '_' || character == '-' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        let previous_lower = index > 0 && chars[index - 1].is_ascii_lowercase();
        let acronym_boundary = index > 0
            && character.is_ascii_uppercase()
            && chars[index - 1].is_ascii_uppercase()
            && chars.get(index + 1).is_some_and(char::is_ascii_lowercase);
        if character.is_ascii_uppercase()
            && (previous_lower || acronym_boundary)
            && !current.is_empty()
        {
            words.push(std::mem::take(&mut current));
        }
        current.push(character);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn capitalize(value: &str) -> String {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    first
        .to_uppercase()
        .chain(characters.flat_map(char::to_lowercase))
        .collect()
}

fn ts_string(value: &str) -> String {
    format!("{value:?}")
}

fn render_readme(definitions: &BTreeMap<String, Definition>) -> String {
    let generated: Vec<_> = generated_definitions(definitions).collect();
    let compatibility_count = generated
        .iter()
        .filter(|definition| definition.origin == Origin::Contract)
        .count();
    let dependency_count = generated.len() - compatibility_count;
    let mut output = format!(
        "# Generated MKVO contracts\n\n\
         `contracts.ts` is generated from the serde-annotated Rust DTOs in `crates/mkvo-contracts`. It currently contains {compatibility_count} boundary DTOs and {dependency_count} referenced domain enums. It also includes zero-dependency runtime validators.\n\n\
         Generate bindings:\n\n\
         ```powershell\n\
         ./scripts/generate-contracts.ps1\n\
         ```\n\n\
         Check drift without writing:\n\n\
         ```powershell\n\
         ./scripts/generate-contracts.ps1 -Check\n\
         # Cross-platform CI equivalent:\n\
         cargo run --locked -p mkvo-contract-gen -- --check\n\
         ```\n\n\
         ## Wire-shape rules\n\n\
         - serde `rename` and `rename_all` determine property and enum names.\n\
         - `Option<T>` is represented as `T | null`. A property is optional when serde can omit it while serializing. On `*Request` DTOs, option fields are also optional because serde accepts a missing option.\n\
         - serde `flatten` is emitted as a TypeScript intersection and validated against the same object.\n\
         - Date/time and ID newtypes are JSON strings; integer primitives are JSON numbers.\n\
         - Unknown JSON values remain the recursive `JsonValue` type.\n\n\
         ## Intentional generation gaps\n\n\
         These Rust-only or domain-native types are deliberately excluded from the compatibility UI boundary:\n\n",
    );
    for (name, reason) in GENERATION_GAPS {
        output.push_str(&format!("- `{name}` — {reason}\n"));
    }
    output.push_str(
        "\nRuntime adapter-only callback types (`BackendClient`, job-progress listener functions, unsubscribe handles, and transport selection) remain hand-maintained in `web/src/backend/client.ts`; they are TypeScript behavior, not serializable Rust DTOs.\n",
    );
    output
}

const VALIDATOR_RUNTIME: &str = r#"export type ContractValidationResult<T> =
  | { ok: true; value: T }
  | { ok: false; errors: string[] };

export function validateContract<Name extends ContractName>(
  name: Name,
  value: unknown
): ContractValidationResult<ContractTypes[Name]> {
  const errors = validateSchema(contractSchemas[name], value, "$", 0);
  return errors.length === 0
    ? { ok: true, value: value as ContractTypes[Name] }
    : { ok: false, errors };
}

export function isContract<Name extends ContractName>(
  name: Name,
  value: unknown
): value is ContractTypes[Name] {
  return validateSchema(contractSchemas[name], value, "$", 0).length === 0;
}

export function assertContract<Name extends ContractName>(
  name: Name,
  value: unknown
): asserts value is ContractTypes[Name] {
  const errors = validateSchema(contractSchemas[name], value, "$", 0);
  if (errors.length !== 0) {
    throw new TypeError(`Invalid ${name}: ${errors.join("; ")}`);
  }
}

function validateSchema(
  schema: ContractSchema,
  value: unknown,
  path: string,
  depth: number
): string[] {
  if (depth > 64) return [`${path}: contract nesting exceeds 64 levels`];
  switch (schema.kind) {
    case "unknown":
      return [];
    case "string":
      return typeof value === "string" ? [] : [`${path}: expected string`];
    case "boolean":
      return typeof value === "boolean" ? [] : [`${path}: expected boolean`];
    case "null":
      return value === null ? [] : [`${path}: expected null`];
    case "literal":
      return value === schema.value ? [] : [`${path}: expected ${JSON.stringify(schema.value)}`];
    case "number": {
      if (typeof value !== "number" || !Number.isFinite(value)) {
        return [`${path}: expected finite number`];
      }
      if (schema.integer && !Number.isInteger(value)) {
        return [`${path}: expected integer`];
      }
      if (schema.unsigned && value < 0) {
        return [`${path}: expected unsigned number`];
      }
      return [];
    }
    case "array":
      if (!Array.isArray(value)) return [`${path}: expected array`];
      return value.flatMap((item, index) =>
        validateSchema(schema.item, item, `${path}[${index}]`, depth + 1)
      );
    case "tuple":
      if (!Array.isArray(value) || value.length !== schema.items.length) {
        return [`${path}: expected tuple with ${schema.items.length} items`];
      }
      return schema.items.flatMap((item, index) =>
        validateSchema(item, value[index], `${path}[${index}]`, depth + 1)
      );
    case "record":
      if (!isRecord(value)) return [`${path}: expected object`];
      return Object.entries(value).flatMap(([key, item]) =>
        validateSchema(schema.value, item, `${path}.${key}`, depth + 1)
      );
    case "ref": {
      const referenced = contractSchemas[schema.name as ContractName];
      return referenced
        ? validateSchema(referenced, value, path, depth + 1)
        : [`${path}: unknown generated schema ${schema.name}`];
    }
    case "union":
      return schema.variants.some(
        (variant) => validateSchema(variant, value, path, depth + 1).length === 0
      )
        ? []
        : [`${path}: did not match any contract variant`];
    case "object": {
      if (!isRecord(value)) return [`${path}: expected object`];
      const errors: string[] = [];
      for (const [field, fieldSchema] of Object.entries(schema.fields)) {
        if (!(field in value)) {
          if (!fieldSchema.optional) errors.push(`${path}.${field}: required field is missing`);
          continue;
        }
        errors.push(
          ...validateSchema(fieldSchema.schema, value[field], `${path}.${field}`, depth + 1)
        );
      }
      for (const flattened of schema.flatten) {
        errors.push(...validateSchema(flattened, value, path, depth + 1));
      }
      return errors;
    }
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casing_matches_serde_rules_used_by_contracts() {
        assert_eq!(apply_case("source_path", Some("camelCase")), "sourcePath");
        assert_eq!(
            apply_case("WaitingForResources", Some("snake_case")),
            "waiting_for_resources"
        );
        assert_eq!(
            apply_case("mkv_tool_nix_directory", Some("camelCase")),
            "mkvToolNixDirectory"
        );
    }

    #[test]
    fn generated_output_covers_casing_optional_flatten_and_status_variants() {
        let generated = generate(&workspace_root()).unwrap().typescript;
        assert!(generated.contains("\"mkvToolNixDirectory\": string | null;"));
        assert!(generated.contains("\"tvdbApiKey\"?: string | null;"));
        // The compatibility layer's OperationJobResponse is what the hosts
        // return, so that is the shape generated; the contracts twin it
        // shadows was the only `#[serde(flatten)]` in the emitted surface.
        assert!(generated.contains("export interface OperationJobResponse {"));
        assert!(
            generated.contains("\"propEditResult\": PropEditPreviewResponse | null;"),
            "the compatibility job response should carry its operation results"
        );
        assert!(generated.contains("\"WaitingForResources\""));
        assert!(generated.contains("\"Skipped\""));
        assert!(generated.contains("export function validateContract"));
    }

    /// Flattening is still supported even though nothing in the emitted
    /// surface uses it today, so the renderer is exercised directly rather
    /// than through whichever contract happens to flatten this week.
    #[test]
    fn a_flattened_field_renders_as_an_intersection() {
        let source = r#"
            #[derive(Serialize)]
            #[serde(rename_all = "camelCase")]
            pub struct Inner { pub left: String }

            #[derive(Serialize)]
            #[serde(rename_all = "camelCase")]
            pub struct Outer {
                #[serde(flatten)]
                pub inner: Inner,
                pub right: String,
            }
        "#;
        let mut definitions = BTreeMap::new();
        for item in syn::parse_file(source).unwrap().items {
            if let Some(definition) = parse_definition(item, Origin::Contract).unwrap() {
                definitions.insert(definition.name.clone(), definition);
            }
        }

        let rendered = render_typescript(&definitions).unwrap();
        assert!(
            rendered.contains("export type Outer = Inner & {"),
            "flattened struct should render as an intersection:
{rendered}"
        );
    }

    #[test]
    fn every_declared_generation_gap_has_a_reason() {
        let definitions = {
            let root = workspace_root();
            let mut definitions = BTreeMap::new();
            for file_name in CONTRACT_FILES {
                load_definitions(
                    &root.join("crates/mkvo-contracts/src").join(file_name),
                    Origin::Contract,
                    None,
                    &mut definitions,
                )
                .unwrap();
            }
            definitions
        };
        validate_coverage(&definitions).unwrap();
    }
}
