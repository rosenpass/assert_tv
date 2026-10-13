//! Derive macro for `assert_tv::TestVectorSet`.
use proc_macro2::Span;
use quote::quote;
use syn::{spanned::Spanned, Attribute, Data, DeriveInput, Error, Fields, LitBool, LitStr, Type};
// -----------------------------------------------------------------------------
// Implementation
// -----------------------------------------------------------------------------

#[allow(dead_code)]
/// Everything we need for one field
struct FieldCfg {
    ident: syn::Ident,
    ty: Type,
    name: Option<String>,
    description: Option<String>,
    serialize_with: Option<syn::Path>,
    deserialize_with: Option<syn::Path>,
    compress: Option<bool>,
    offload: Option<bool>,
    span: Span,
}

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    // 1. Accept only structs with named fields
    let struct_name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(f) => &f.named,
            _ => {
                return Err(Error::new_spanned(
                    &input.ident,
                    "TestVectorSet can only be derived for structs with named fields",
                ))
            }
        },
        _ => {
            return Err(Error::new_spanned(
                &input.ident,
                "TestVectorSet can only be derived for structs",
            ))
        }
    };

    // 2. Walk each field and collect configuration
    let mut cfgs = Vec::<FieldCfg>::with_capacity(fields.len());

    for field in fields {
        let span = field.span();
        let ident = field
            .ident
            .clone()
            .ok_or_else(|| Error::new(span, "unnamed field – only named fields are supported"))?;

        // ensure field type is `TestValue<..>`
        ensure_test_value_type(&field.ty)?;

        let mut name = None;
        let mut description = None;
        let mut serialize_with = None;
        let mut deserialize_with = None;
        let mut compress = None;
        let mut offload = None;

        for attr in &field.attrs {
            if !attr.path().is_ident("test_vec") {
                continue;
            }

            parse_test_vec_attribute(
                attr,
                &mut name,
                &mut description,
                &mut serialize_with,
                &mut deserialize_with,
                &mut compress,
                &mut offload,
            )?;
        }

        cfgs.push(FieldCfg {
            ident,
            ty: field.ty.clone(),
            name,
            description,
            serialize_with,
            deserialize_with,
            compress,
            offload,
            span,
        });
    }

    // 3. Generate the body of `Self { ... }`
    let field_inits = cfgs.iter().map(|f| {
        let ident = &f.ident;
        let name = opt_string(&f.name);
        let description = opt_string(&f.description);
        let compress = opt_bool_default_false(&f.compress);
        let offload = opt_bool_default_false(&f.offload);

        let serializer = if let Some(path) = &f.serialize_with {
            quote! {
                if TV::is_test_vector_enabled() {
                    Some(::std::boxed::Box::new(#path))
                } else {
                    None
                }
            }
        } else {
            // default serde_json serializer
            quote! {
                if TV::is_test_vector_enabled() {
                    Some(::std::boxed::Box::new(|v| ::serde_json::to_value(v).map_err(::anyhow::Error::from)))
                } else {
                    None
                }
            }
        };

        let deserializer = if let Some(path) = &f.deserialize_with {
            quote! {
                if TV::is_test_vector_enabled() {
                    Some(::std::boxed::Box::new(#path))
                } else {
                    None
                }
            }
        } else {
            quote! {
                if TV::is_test_vector_enabled() {
                    Some(::std::boxed::Box::new(|v| ::serde_json::from_value(v.clone()).map_err(::anyhow::Error::from)))
                } else {
                    None
                }
            }
        };

        quote! {
            #ident: ::assert_tv::TestValue {
                name: #name,
                description: #description,
                test_value_field_code_location: format!("{}:{}", ::core::file!(), ::core::line!()),
                serializer: #serializer,
                deserializer: #deserializer,
                compress: #compress,
                offload: #offload,
                _data_marker: ::core::default::Default::default(),
            }
        }
    });

    let expanded = quote! {
        impl #impl_generics ::assert_tv::TestVectorSet for #struct_name #ty_generics #where_clause {
            fn start<TV: ::assert_tv::TestVector>() -> Self {
                Self {
                    #(#field_inits),*
                }
            }
        }
    };

    Ok(expanded)
}

// --- helpers ---------------------------------------------------------------

fn opt_string(opt: &Option<String>) -> proc_macro2::TokenStream {
    match opt {
        Some(s) => quote! { Some(::std::string::String::from(#s)) },
        None => quote! { None },
    }
}

fn opt_bool_default_false(opt: &Option<bool>) -> proc_macro2::TokenStream {
    match opt {
        Some(b) => quote! { #b },
        None => quote! { #false },
    }
}

/// Verify the field is of type `TestValue<...>`
fn ensure_test_value_type(ty: &Type) -> syn::Result<()> {
    if let Type::Path(p) = ty
        && let Some(seg) = p.path.segments.last()
        && seg.ident == "TestValue"
    {
        return Ok(());
    }
    Err(Error::new_spanned(
        ty,
        "all fields in a TestVectorSet must be of type `TestValue<…>`",
    ))
}
/// Parses a single `#[test_vec(...)]` attribute using the syn 2 helper API.
///
/// Accepted keys are
///  * `name = "…"`,
///  * `description = "…"`,
///  * `serialize_with = "path::to::fn"`,
///  * `deserialize_with = "path::to::fn"`.
///
/// Any other key or any duplicate key results in a descriptive compile‑error.
pub fn parse_test_vec_attribute(
    attr: &Attribute,
    name: &mut Option<String>,
    description: &mut Option<String>,
    serialize_with: &mut Option<syn::Path>,
    deserialize_with: &mut Option<syn::Path>,
    compress: &mut Option<bool>,
    offload: &mut Option<bool>,
) -> syn::Result<()> {
    let _ = compress;
    attr.parse_nested_meta(|meta| {
        // ---- helper -------------------------------------------------------
        let get_lit_str = || -> syn::Result<LitStr> {
            meta.value()?
                .parse()
                .map_err(|e: Error| Error::new(meta.path.span(), e.to_string()))
        };
        let get_lit_bool = || -> syn::Result<LitBool> {
            meta.value()?
                .parse()
                .map_err(|e: Error| Error::new(meta.path.span(), e.to_string()))
        };
        // -------------------------------------------------------------------

        if meta.path.is_ident("name") {
            let lit: LitStr = get_lit_str()?;
            if name.replace(lit.value()).is_some() {
                return Err(meta.error("duplicate `name` key"));
            }
            return Ok(());
        }

        if meta.path.is_ident("description") {
            let lit: LitStr = get_lit_str()?;
            if description.replace(lit.value()).is_some() {
                return Err(meta.error("duplicate `description` key"));
            }
            return Ok(());
        }

        if meta.path.is_ident("serialize_with") {
            let lit: LitStr = get_lit_str()?;
            let path: syn::Path = syn::parse_str(&lit.value())?;
            if serialize_with.replace(path).is_some() {
                return Err(meta.error("duplicate `serialize_with` key"));
            }
            return Ok(());
        }

        if meta.path.is_ident("deserialize_with") {
            let lit: LitStr = get_lit_str()?;
            let path: syn::Path = syn::parse_str(&lit.value())?;
            if deserialize_with.replace(path).is_some() {
                return Err(meta.error("duplicate `deserialize_with` key"));
            }
            return Ok(());
        }

        // if meta.path.is_ident("compress") {
        //     let lit: LitBool = get_lit_bool()?;
        //     if compress.replace(lit.value()).is_some() {
        //         return Err(meta.error("duplicate `compress` key"));
        //     }
        // }
        if meta.path.is_ident("offload") {
            let lit: LitBool = get_lit_bool()?;
            if offload.replace(lit.value()).is_some() {
                return Err(meta.error("duplicate `offload` key"));
            }
            return Ok(());
        }

        Err(meta.error(
            "unrecognised key; allowed: name, description, serialize_with, deserialize_with, offload",
        ))
    })
}
