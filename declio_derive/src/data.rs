use darling::{ast, Error, FromDeriveInput, FromField, FromMeta, FromVariant};
use quote::{format_ident, quote, ToTokens};
use syn::parse_quote;

#[derive(FromDeriveInput)]
#[darling(attributes(declio))]
pub struct Container {
    ident: syn::Ident,
    generics: syn::Generics,
    data: ast::Data<Variant, Field>,

    #[darling(default)]
    crate_path: Option<syn::Path>,
}

impl Container {
    pub fn impl_encode(self) -> Result<syn::ItemImpl, Error> {
        let Self {
            ident,
            generics,
            data,
            crate_path,
        } = self;

        let crate_path = crate_path.unwrap_or_else(|| parse_quote!(declio));
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let writer_binding: syn::Ident = parse_quote!(writer);

        let variants = match data {
            ast::Data::Enum(variants) => variants,
            ast::Data::Struct(fields) => vec![Variant::from_struct(fields)],
        };
        let variant_arms = variants.into_iter().map(|var| {
            var.generate_encode_arm(&crate_path, &writer_binding)
                .map(ToTokens::into_token_stream)
                .unwrap_or_else(Error::write_errors)
        });

        Ok(parse_quote! {
            impl #impl_generics #crate_path::Encode<()> for #ident #ty_generics
                #where_clause
            {
                fn encode<W>(&self, _: (), #writer_binding: &mut W)
                    -> Result<(), #crate_path::export::io::Error>
                where
                    W: #crate_path::export::io::Write,
                {
                    match self {
                        #( #variant_arms )*
                    }
                }
            }
        })
    }

    pub fn impl_decode(self) -> Result<syn::ItemImpl, Error> {
        let Self {
            ident,
            generics,
            data,
            crate_path,
        } = self;

        let crate_path = crate_path.unwrap_or_else(|| parse_quote!(declio));
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let reader_binding: syn::Ident = parse_quote!(reader);

        let variants = match data {
            ast::Data::Enum(variants) => variants,
            ast::Data::Struct(fields) => vec![Variant::from_struct(fields)],
        };
        let variant_arms = variants.into_iter().map(|var| {
            var.generate_decode_arm(&crate_path, &reader_binding)
                .map(ToTokens::into_token_stream)
                .unwrap_or_else(Error::write_errors)
        });

        Ok(parse_quote! {
            impl #impl_generics #crate_path::Decode<()> for #ident #ty_generics
                #where_clause
        {
                fn decode<R>(_: (), reader: &mut R)
                    -> Result<(), #crate_path::export::io::Error>
                where
                    R: #crate_path::export::io::Read,
                {
                    let id = todo!();
                    match id {
                        #( #variant_arms )*
                    }
                }
            }
        })
    }
}

#[derive(FromVariant)]
#[darling(attributes(declio))]
pub struct Variant {
    ident: syn::Ident,
    fields: ast::Fields<Field>,

    #[darling(skip)]
    from_struct: bool,
}

impl Variant {
    pub fn from_struct(fields: ast::Fields<Field>) -> Self {
        Self {
            ident: parse_quote!(__declio_unused),
            fields,
            from_struct: true,
        }
    }

    pub fn generate_encode_arm(
        self,
        crate_path: &syn::Path,
        writer_binding: &syn::Ident,
    ) -> Result<syn::Arm, Error> {
        let Self {
            ident,
            fields,
            from_struct,
        } = self;

        let path: syn::Path;
        if from_struct {
            path = parse_quote!(Self);
        } else {
            path = parse_quote!(Self::#ident);
        }

        let pat_fields_inner = fields
            .iter()
            .enumerate()
            .map(|(index, field)| field.ident(index));
        let pat_fields = match fields.style {
            ast::Style::Tuple => quote! {
                ( #( #pat_fields_inner, )* )
            },
            ast::Style::Struct => quote! {
                { #( #pat_fields_inner, )* }
            },
            ast::Style::Unit => quote! {},
        };

        let field_encoders = fields.iter().enumerate().map(|(index, field)| {
            field
                .generate_encoder(&crate_path, &field.ident(index), writer_binding)
                .map(ToTokens::into_token_stream)
                .unwrap_or_else(Error::write_errors)
        });

        Ok(parse_quote! {
            #path #pat_fields => {
                #( #field_encoders ?; )*
                Ok(())
            }
        })
    }

    pub fn generate_decode_arm(
        self,
        crate_path: &syn::Path,
        reader_binding: &syn::Ident,
    ) -> Result<syn::Arm, Error> {
        let Self {
            ident,
            fields,
            from_struct,
        } = self;

        let path: syn::Path;
        let id_pat: syn::Pat;
        if from_struct {
            path = parse_quote!(Self);
            id_pat = parse_quote!(_);
        } else {
            path = parse_quote!(Self::#ident);
            id_pat = todo!();
        }

        let cons_fields_inner = fields
            .iter()
            .enumerate()
            .map(|(index, field)| field.ident(index));
        let cons_fields = match fields.style {
            ast::Style::Tuple => quote! {
                ( #( #cons_fields_inner, )* )
            },
            ast::Style::Struct => quote! {
                { #( #cons_fields_inner, )* }
            },
            ast::Style::Unit => quote! {},
        };

        let field_decoders = fields.iter().enumerate().map(|(index, field)| {
            let binding = field.ident(index);
            let decoder = field
                .generate_decoder(&crate_path, reader_binding)
                .map(ToTokens::into_token_stream)
                .unwrap_or_else(Error::write_errors);
            quote! { let #binding = #decoder ?; }
        });

        Ok(parse_quote! {
            #id_pat => {
                #( #field_decoders )*
                Ok(#path #cons_fields)
            }
        })
    }
}

#[derive(FromField)]
#[darling(attributes(declio))]
pub struct Field {
    ident: Option<syn::Ident>,
    ty: syn::Type,
}

impl Field {
    pub fn ident(&self, index: usize) -> syn::Ident {
        self.ident
            .clone()
            .unwrap_or_else(|| format_ident!("field_{}", index))
    }

    pub fn generate_encoder(
        &self,
        crate_path: &syn::Path,
        binding: &syn::Ident,
        writer_binding: &syn::Ident,
    ) -> Result<syn::Expr, Error> {
        let Self { ty, .. } = self;
        Ok(parse_quote! { <#ty as #crate_path::Encode>::encode(#binding, (), #writer_binding) })
    }

    pub fn generate_decoder(
        &self,
        crate_path: &syn::Path,
        reader_binding: &syn::Ident,
    ) -> Result<syn::Expr, Error> {
        let Self { ty, .. } = self;
        Ok(parse_quote! { <#ty as #crate_path::Decode>::decode((), #reader_binding) })
    }
}

pub enum Asym<T> {
    Single(T),
    Multi {
        encode: Option<T>,
        decode: Option<T>,
    },
}

impl<T> Asym<T> {
    pub fn encode(self) -> Option<T> {
        match self {
            Self::Single(val) => Some(val),
            Self::Multi { encode, .. } => encode,
        }
    }

    pub fn decode(self) -> Option<T> {
        match self {
            Self::Single(val) => Some(val),
            Self::Multi { decode, .. } => decode,
        }
    }
}

impl<T> FromMeta for Asym<T>
where
    T: FromMeta,
{
    fn from_meta(item: &syn::Meta) -> Result<Self, Error> {
        match item {
            syn::Meta::List(value) => {
                Self::from_list(&value.nested.iter().cloned().collect::<Vec<_>>())
            }
            _ => T::from_meta(item).map(Self::Single),
        }
    }

    fn from_list(items: &[syn::NestedMeta]) -> Result<Self, Error> {
        let mut encode = None;
        let mut decode = None;

        let encode_path: syn::Path = parse_quote!(encode);
        let decode_path: syn::Path = parse_quote!(decode);

        for item in items {
            match item {
                syn::NestedMeta::Meta(meta) => match meta.path() {
                    path if *path == encode_path => {
                        if encode.is_none() {
                            encode = Some(T::from_meta(meta)?);
                        } else {
                            return Err(Error::duplicate_field_path(path));
                        }
                    }
                    path if *path == decode_path => {
                        if decode.is_none() {
                            decode = Some(T::from_meta(meta)?);
                        } else {
                            return Err(Error::duplicate_field_path(path));
                        }
                    }
                    other => return Err(Error::unknown_field_path(other)),
                },
                syn::NestedMeta::Lit(..) => return Err(Error::unsupported_format("literal")),
            }
        }
        Ok(Self::Multi { encode, decode })
    }
}

impl<T> Default for Asym<T>
where
    T: Default,
{
    fn default() -> Self {
        Self::Single(Default::default())
    }
}
