use crate::prelude::*;

pub(super) fn run(input: syn::DeriveInput) -> TokenStream {
    let span = input.span();
    let ty = input.ident;
    let generics = input.generics;
    let mut diag = TokenStream::new();

    let result = match input.data {
        syn::Data::Struct(s) => {
            try_impl_struct(span, input.attrs, ty, generics, s.fields, &mut diag)
        },
        syn::Data::Enum(e) => try_impl_enum(span, ty, generics, e.variants, &mut diag),
        _ => span
            .error("Triage can only be derived on struct or enum types")
            .into_compile_error(),
    };

    [diag, result].into_iter().collect()
}

fn try_impl_struct(
    span: Span,
    attrs: Vec<syn::Attribute>,
    ty: syn::Ident,
    generics: syn::Generics,
    fields: syn::Fields,
    diag: &mut TokenStream,
) -> TokenStream {
    let (fields, severity) = parse_fields(span, attrs, fields, diag);

    let body = quote_spanned! { span =>
        let Self #fields = *self;
        #severity
    };

    triage(span, ty, generics, body)
}

fn try_impl_enum(
    span: Span,
    ty: syn::Ident,
    generics: syn::Generics,
    vars: syn::punctuated::Punctuated<syn::Variant, syn::Token![,]>,
    diag: &mut TokenStream,
) -> TokenStream {
    let vars: Vec<TokenStream> = vars
        .into_iter()
        .map(|v| {
            let span = v.span();
            let id = v.ident;

            let (fields, severity) = parse_fields(span, v.attrs, v.fields, diag);
            quote_spanned! { span => Self::#id #fields => #severity }
        })
        .collect();

    let body = quote_spanned! { span => match *self { #(#vars,)* } };

    triage(span, ty, generics, body)
}

fn parse_fields(
    span: Span,
    attrs: Vec<syn::Attribute>,
    fields: syn::Fields,
    diag: &mut TokenStream,
) -> (TokenStream, TokenStream) {
    const TRANSIENT: &str = "Transient";
    const PERMANENT: &str = "Permanent";
    const FATAL: &str = "Fatal";

    let explicit = attrs.into_iter().fold(None, |d, a| {
        let span = a.span();

        let next = match a
            .meta
            .path()
            .get_ident()
            .map(ToString::to_string)
            .as_deref()
        {
            Some("transient") => Some(TRANSIENT),
            Some("permanent") => Some(PERMANENT),
            Some("fatal") => Some(FATAL),
            _ => None,
        };

        if d.is_some() && next.is_some() {
            diag.extend(
                span.error("Duplicate severity attribute")
                    .into_compile_error(),
            );
        }

        next.or(d)
    });

    let source = fields.iter().enumerate().fold(None, |s, (i, f)| {
        let span = f.span();

        let next = if f.attrs.iter().any(|a| {
            matches!(
                a.meta
                    .path()
                    .get_ident()
                    .map(ToString::to_string)
                    .as_deref(),
                Some("source" | "from")
            )
        }) {
            Some(i)
        } else {
            None
        };

        if s.is_some() && next.is_some() {
            diag.extend(
                span.error("Multiple fields marked with #[source]")
                    .into_compile_error(),
            );
        }

        next.or(s)
    });

    let destructured = proc_macro2::Ident::new("__triage_f", span);
    let (severity, field) = match (explicit, source) {
        (s, None) | (s @ Some(_), Some(_)) => {
            let id = proc_macro2::Ident::new(s.unwrap_or(PERMANENT), span);
            (quote_spanned! { span => Severity::#id }, None)
        },
        (None, Some(f)) => (
            quote_spanned! { span => Triage::severity(#destructured) },
            Some(f),
        ),
    };

    let fields = match fields {
        syn::Fields::Named(n) => {
            let span = n.span();

            let field = field.map(|f| {
                let id = n
                    .named
                    .into_iter()
                    .nth(f)
                    .and_then(|f| f.ident)
                    .unwrap_or_else(|| unreachable!());

                quote_spanned! { span => #id: ref #destructured, }
            });

            quote_spanned! { span => { #field .. } }
        },
        syn::Fields::Unnamed(u) => {
            let span = u.span();

            let fields = u.unnamed.into_iter().enumerate().map(|(i, f)| {
                let span = f.span();

                match field {
                    Some(f) if i == f => quote_spanned! { span => ref #destructured },
                    _ => quote_spanned! { span => _ },
                }
            });

            quote_spanned! { span => (#(#fields),*) }
        },
        syn::Fields::Unit => TokenStream::new(),
    };

    (fields, severity)
}

fn triage(span: Span, ty: syn::Ident, generics: syn::Generics, body: TokenStream) -> TokenStream {
    let (impl_gen, ty_gen, where_toks) = generics.split_for_impl();
    quote_spanned! { span =>
        impl #impl_gen Triage for #ty #ty_gen #where_toks {
            #[inline]
            fn severity(&self) -> Severity { #body }
        }
    }
}
