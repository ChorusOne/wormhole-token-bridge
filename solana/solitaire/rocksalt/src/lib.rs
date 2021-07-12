#![allow(warnings)]

mod to_instruction;

use to_instruction::*;

use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    pubkey::Pubkey,
};

use proc_macro::TokenStream;
use proc_macro2::{
    Span,
    TokenStream as TokenStream2,
};
use quote::{
    quote,
    quote_spanned,
    ToTokens,
};
use std::borrow::BorrowMut;
use syn::{
    parse_macro_input,
    parse_quote,
    spanned::Spanned,
    Data,
    DataStruct,
    DeriveInput,
    Fields,
    GenericParam,
    Generics,
    Index,
};

#[proc_macro_derive(ToInstruction)]
pub fn derive_to_instruction(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    // Type params of the instruction context account
    let type_params: Vec<GenericParam> = input
        .generics
        .type_params()
        .map(|v| GenericParam::Type(v.clone()))
        .collect();

    // Generics lifetimes of the peel type
    let mut peel_g = input.generics.clone();
    peel_g.params = parse_quote!('a, 'b: 'a, 'c);
    let (_, peel_type_g, _) = peel_g.split_for_impl();

    // Params of the instruction context
    let mut type_generics = input.generics.clone();
    type_generics.params = parse_quote!('b);
    for x in &type_params {
        type_generics.params.push(x.clone());
    }
    let (type_impl_g, type_g, _) = type_generics.split_for_impl();

    // Combined lifetimes of peel and the instruction context
    let mut combined_generics = Generics::default();
    combined_generics.params = peel_g.params.clone();
    for x in &type_params {
        combined_generics.params.push(x.clone());
    }
    let (combined_impl_g, _, _) = combined_generics.split_for_impl();

    let expanded = generate_to_instruction(&name, &combined_impl_g, &input.data);
    TokenStream::from(expanded)
}

/// Generate a FromAccounts implementation for a product of accounts. Each field is constructed by
/// a call to the Verify::verify instance of its type.
#[proc_macro_derive(FromAccounts)]
pub fn derive_from_accounts(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    // Type params of the instruction context account
    let type_params: Vec<GenericParam> = input
        .generics
        .type_params()
        .map(|v| GenericParam::Type(v.clone()))
        .collect();

    // Generics lifetimes of the peel type
    let mut peel_g = input.generics.clone();
    peel_g.params = parse_quote!('a, 'b: 'a, 'c);
    let (_, peel_type_g, _) = peel_g.split_for_impl();

    // Params of the instruction context
    let mut type_generics = input.generics.clone();
    type_generics.params = parse_quote!('b);
    for x in &type_params {
        type_generics.params.push(x.clone());
    }
    let (type_impl_g, type_g, _) = type_generics.split_for_impl();

    // Combined lifetimes of peel and the instruction context
    let mut combined_generics = Generics::default();
    combined_generics.params = peel_g.params.clone();
    for x in &type_params {
        combined_generics.params.push(x.clone());
    }
    let (combined_impl_g, _, _) = combined_generics.split_for_impl();

    let from_method = generate_from(&name, &input.data);
    let to_cpi_metas_method = generate_to_cpi_metas(&name, &input.data);
    let size_in_accounts_method = generate_size_in_accounts(&name, &input.data);
    let persist_method = generate_persist(&name, &input.data);
    let deps_method = generate_deps_fields(&name, &input.data);
    let expanded = quote! {
        /// Macro generated implementation of FromAccounts by Solitaire.
        impl #combined_impl_g solitaire::FromAccounts #peel_type_g for #name #type_g {
            fn from<DataType>(pid: &'a solana_program::pubkey::Pubkey, iter: &'c mut std::slice::Iter<'a, solana_program::account_info::AccountInfo<'b>>, data: &'a DataType) -> solitaire::Result<Self> {
                #from_method
            }
        fn size_in_accounts() -> usize {
        #size_in_accounts_method
        }
	    fn to_cpi_metas(infos: &'c mut std::slice::Iter<Info<'b>>) -> solitaire::Result<Vec<solana_program::instruction::AccountMeta>> {
                #to_cpi_metas_method
            }
        }

        /// Macro generated implementation of Persist by Solitaire.
        impl #type_impl_g solitaire::Persist for #name #type_g {
            fn persist(&self, program_id: &solana_program::pubkey::Pubkey) -> solitaire::Result<()> {
                #persist_method
            }
        }
    };

    // Hand the output tokens back to the compiler
    TokenStream::from(expanded)
}

/// This function does the heavy lifting of generating the field parsers.
fn generate_from(name: &syn::Ident, data: &Data) -> TokenStream2 {
    match *data {
        // We only care about structures.
        Data::Struct(ref data) => {
            // We want to inspect its fields.
            match data.fields {
                // For now, we only care about struct { a: T } forms, not struct(T);
                Fields::Named(ref fields) => {
                    // For each field, generate an expression that parses an account info field
                    // from the Solana accounts list. This relies on Verify::verify to do most of
                    // the work.
                    let recurse = fields.named.iter().map(|f| {
                        // Field name, to assign to.
                        let name = &f.ident;
                        let ty = &f.ty;

                        quote! {
                            trace!(stringify!(#name));
                            let #name: #ty = solitaire::Peel::peel(&mut solitaire::Context::new(
                                pid,
                                iter,
                                data,
                            ))?;
                        }
                    });

                    let names = fields.named.iter().map(|f| {
                        let name = &f.ident;
                        quote!(#name)
                    });

                    // Write out our iterator and return the filled structure.
                    quote! {
                        use solana_program::account_info::next_account_info;
                        use solitaire::trace;
                        trace!("Peeling:");
                        #(#recurse;)*
                        Ok(#name { #(#names,)* })
                    }
                }

                Fields::Unnamed(_) => {
                    unimplemented!()
                }

                Fields::Unit => {
                    unimplemented!()
                }
            }
        }

        Data::Enum(_) | Data::Union(_) => unimplemented!(),
    }
}

fn generate_to_cpi_metas(name: &syn::Ident, data: &Data) -> TokenStream2 {
    if let Data::Struct(DataStruct {
        fields: Fields::Named(fields),
        ..
    }) = data
    {
        let v_appends = fields.named.iter().map(|f| {
            // Field name, to assign to.
            let field_name = &f.ident;
            let field_ty = &f.ty;

            quote! {
                v.append(&mut <#field_ty as Peel>::to_partial_cpi_metas(infos)?);
            }
        });

        let names = fields.named.iter().map(|f| {
            let name = &f.ident;
            quote!(#name)
        });

        quote! {
            let mut v = Vec::new();
            #(#v_appends;)*
	    if v.len() != Self::size_in_accounts() {
		return Err(solitaire::SolitaireError::ProgramError(solana_program::program_error::ProgramError::InvalidAccountData));
	    }
	    Ok(v) 
	}
    } else {
        unimplemented!()
    }
}

/// This function does the heavy lifting of generating the field parsers.
fn generate_deps_fields(name: &syn::Ident, data: &Data) -> TokenStream2 {
    match *data {
        // We only care about structures.
        Data::Struct(ref data) => {
            // We want to inspect its fields.
            match data.fields {
                // For now, we only care about struct { a: T } forms, not struct(T);
                Fields::Named(ref fields) => {
                    // For each field, generate an expression appends it deps
                    let recurse = fields.named.iter().map(|f| {
                        let ty = &f.ty;
                        quote! {
                            deps.append(&mut <#ty as Peel>::deps());
                        }
                    });

                    // Write out our iterator and return the filled structure.
                    quote! {
                        let mut deps = Vec::new();
                        #(#recurse;)*
                        deps
                    }
                }

                Fields::Unnamed(_) => {
                    unimplemented!()
                }

                Fields::Unit => {
                    unimplemented!()
                }
            }
        }

        Data::Enum(_) | Data::Union(_) => unimplemented!(),
    }
}

/// This function does the heavy lifting of generating the field parsers.
fn generate_persist(name: &syn::Ident, data: &Data) -> TokenStream2 {
    match *data {
        // We only care about structures.
        Data::Struct(ref data) => {
            // We want to inspect its fields.
            match data.fields {
                // For now, we only care about struct { a: T } forms, not struct(T);
                Fields::Named(ref fields) => {
                    let recurse = fields.named.iter().map(|f| {
                        // Field name, to assign to.
                        let name = &f.ident;
                        let ty = &f.ty;

                        quote! {
                            trace!(stringify!(#name));
                            Peel::persist(&self.#name, program_id)?;
                        }
                    });

                    // Write out our iterator and return the filled structure.
                    quote! {
                        use solitaire::trace;
                        trace!("Persisting:");
                        #(#recurse;)*
                        Ok(())
                    }
                }

                Fields::Unnamed(_) => {
                    unimplemented!()
                }

                Fields::Unit => {
                    unimplemented!()
                }
            }
        }

        Data::Enum(_) | Data::Union(_) => unimplemented!(),
    }
}

fn generate_size_in_accounts(name: &syn::Ident, data: &Data) -> TokenStream2 {
    if let Data::Struct(DataStruct {
        fields: Fields::Named(fields),
        ..
    }) = data
    {
        // For each field, call the relevant partial_size_in_accounts method
        let size_additions = fields.named.iter().map(|f| {
            // Field name, to assign to.
            let field_name = &f.ident;
            let ty = &f.ty;

            quote! {
            size += <#ty as Peel>::partial_size_in_accounts(); // #field_name
                }
        });

        quote! {
            let mut size = 0;
            #(#size_additions;)*
            size
        }
    } else {
        unimplemented!()
    }
}