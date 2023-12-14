// Copyright (c) 2023 the Hearth contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// This file is part of Hearth.
//
// Hearth is free software: you can redistribute it and/or modify it under the
// terms of the GNU Affero General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// Hearth is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License for more
// details.
//
// You should have received a copy of the GNU Affero General Public License
// along with Hearth. If not, see <https://www.gnu.org/licenses/>.

use proc_macro2::{Literal, Span, TokenStream};
use quote::quote;
use syn::{
    parse_macro_input, AttributeArgs, FnArg, Ident, ImplItem, ImplItemMethod, Meta, MetaNameValue,
    NestedMeta, Pat, PatIdent, Type,
};

#[proc_macro_attribute]
pub fn impl_wasm_linker(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let args = parse_macro_input!(attr as AttributeArgs);
    let impl_item = parse_macro_input!(item as syn::ItemImpl);

    let fn_items = impl_item.items;
    let impl_type = impl_item.self_ty;

    let module = args
        .into_iter()
        .next()
        .expect("Expected one attribute argument");
    let module = match module {
        NestedMeta::Meta(Meta::NameValue(MetaNameValue { path, lit, .. })) => {
            let path = path.get_ident().expect("Argument key must be ident");
            assert_eq!(
                path.to_string(),
                "module",
                "Only supported argument is 'module'"
            );
            lit
        }
        _ => panic!("Set only the module with 'module = \"your module\""),
    };

    let mut items_within_impl = vec![];
    let mut link_wrapped_fns = vec![];
    let mut wasm_linker_fns = vec![];
    for fn_item in fn_items {
        items_within_impl.push(quote! {
            #fn_item
        });
        handle_fn_item(
            &mut link_wrapped_fns,
            &mut wasm_linker_fns,
            impl_type.clone(),
            fn_item,
        );
    }

    quote! {
        impl #impl_type {
            const MODULE: &'static str = #module;

            #(#items_within_impl)*
            #(#link_wrapped_fns)*
        }
        impl <T: GetAbi<#impl_type> + Send + 'static> WasmLinker<T> for #impl_type {
            fn add_to_linker(linker: &mut Linker<T>) {
                #(#wasm_linker_fns)*
            }
        }
    }
    .into()
}
fn handle_fn_item(
    link_wrapped_fns: &mut Vec<TokenStream>,
    wasm_linker_fns: &mut Vec<TokenStream>,
    impl_type: Box<Type>,
    fn_item: ImplItem,
) {
    let fn_method = get_fn_method(fn_item);
    let impl_type = get_impl_type_ident(impl_type);
    let link_fn_ident = get_link_fn_ident(&fn_method);

    let linker_function = generate_linker_function(&link_fn_ident, &fn_method, &impl_type);
    let wasm_linker_fn = generate_add_to_linker_call(&link_fn_ident);
    link_wrapped_fns.push(linker_function);
    wasm_linker_fns.push(wasm_linker_fn);
}
fn generate_linker_function(
    link_fn_ident: &Ident,
    fn_method: &ImplItemMethod,
    impl_type: &Ident,
) -> TokenStream {
    let link_fn_ident = link_fn_ident.clone();
    let internal_function = generate_internal_function(fn_method, impl_type);
    let func_wrap_call = generate_func_wrap(fn_method);
    quote! {
        pub fn #link_fn_ident<T: GetAbi<Self> + Send>(linker: &mut Linker<T>) {
            #internal_function
            #func_wrap_call
        }
    }
}
fn generate_internal_function(fn_method: &ImplItemMethod, impl_type: &Ident) -> TokenStream {
    let impl_type = impl_type.clone();
    let fn_name = get_fn_name(fn_method);
    let internal_args = get_internal_args(fn_method);
    let internal_parameters = get_internal_parameters(fn_method);
    let return_type = fn_method.sig.output.clone();
    if is_async(fn_method) {
        quote! {
            async fn #fn_name <T: GetAbi<#impl_type> + Send>(#internal_args) #return_type {
                let this = caller.data_mut().get_abi()?;
                this.#fn_name(#internal_parameters).await
            }
        }
    } else {
        quote! {
            fn #fn_name <T: GetAbi<#impl_type> + Send>(#internal_args) #return_type {
                let this = caller.data_mut().get_abi()?;
                this.#fn_name(#internal_parameters)
            }
        }
    }
}
fn generate_add_to_linker_call(link_fn_ident: &Ident) -> TokenStream {
    let link_fn_ident = link_fn_ident.clone();
    quote! {
        Self::#link_fn_ident(linker);
    }
}
fn generate_func_wrap(fn_method: &ImplItemMethod) -> TokenStream {
    let func_wrap_ident = generate_func_wrap_ident(fn_method);
    let fn_literal = get_func_wrap_literal(fn_method);
    let closure_call_params = get_internal_parameters(fn_method);
    let closure_args = generate_closure_args(fn_method);
    let internal_fn_name = get_fn_name(fn_method);
    let fn_call_thing = if is_async(fn_method) {
        quote! {
            Box::new(#internal_fn_name(caller, #closure_call_params))
        }
    } else {
        quote! {
            #internal_fn_name(caller, #closure_call_params)
        }
    };
    if has_guest_memory(&get_fn_args(fn_method)) {
        quote! {
            linker.#func_wrap_ident(Self::MODULE, #fn_literal, |#closure_args| {
                // if constructing GuestMemory fails something is seriously wrong
                let memory = GuestMemory::from_caller(&mut caller).unwrap();

                #fn_call_thing
            }).unwrap();
        }
    } else {
        quote! {
            linker.#func_wrap_ident(Self::MODULE, #fn_literal, |#closure_args| {
                #fn_call_thing
            }).unwrap();
        }
    }
}
fn generate_closure_args(fn_method: &ImplItemMethod) -> TokenStream {
    let caller_arg = quote! {
      mut caller: Caller<'_, T>
    };
    let fn_args = remove_guest_memory_if_exists(get_fn_args(fn_method));
    quote! {
        #caller_arg, #(#fn_args),*
    }
}
fn generate_func_wrap_ident(fn_method: &ImplItemMethod) -> Ident {
    let mut num_args = get_fn_args(fn_method).len();
    if has_guest_memory(&get_fn_args(fn_method)) {
        num_args -= 1;
    }
    let str = if is_async(fn_method) {
        format!("func_wrap{num_args}_async")
    } else {
        String::from("func_wrap")
    };
    Ident::new(str.as_str(), Span::call_site())
}
fn get_internal_args(fn_method: &ImplItemMethod) -> TokenStream {
    let caller_arg = quote! {
      mut caller: Caller<'_, T>
    };
    let fn_args = get_fn_args(fn_method);
    quote! {
        #caller_arg, #(#fn_args),*
    }
}
fn get_internal_parameters(fn_method: &ImplItemMethod) -> TokenStream {
    let args = get_fn_args(fn_method);
    let args: Vec<_> = args
        .into_iter()
        .map(|arg| match arg {
            FnArg::Receiver(_) => panic!(),
            FnArg::Typed(typed) => match typed.pat.as_ref() {
                Pat::Ident(ident) => Pat::Ident(PatIdent {
                    attrs: vec![],
                    by_ref: None,
                    mutability: None,
                    ident: ident.ident.clone(),
                    subpat: None,
                }),
                _ => panic!(),
            },
        })
        .collect();
    quote! {
        #(#args),*
    }
}
fn get_link_fn_ident(fn_method: &ImplItemMethod) -> Ident {
    let fn_name = get_fn_name(fn_method);
    let str = format!("link_{fn_name}");
    Ident::new(str.as_str(), Span::call_site())
}
fn get_fn_name(fn_method: &ImplItemMethod) -> Ident {
    fn_method.sig.ident.clone()
}
fn get_func_wrap_literal(fn_method: &ImplItemMethod) -> Literal {
    Literal::string(fn_method.sig.ident.to_string().as_str())
}
fn get_fn_args(fn_method: &ImplItemMethod) -> Vec<FnArg> {
    let mut args: Vec<FnArg> = fn_method.sig.inputs.iter().cloned().collect();
    // removing the 'self' parameter
    args.remove(0);
    args
}
fn get_impl_type_ident(impl_type: Box<Type>) -> Ident {
    match impl_type.as_ref() {
        Type::Path(path) => path.path.get_ident().unwrap().clone(),
        _ => panic!(),
    }
}
fn has_guest_memory(fn_args: &Vec<FnArg>) -> bool {
    for fn_arg in fn_args {
        match fn_arg {
            FnArg::Receiver(_) => {}
            FnArg::Typed(typed) => {
                if let Type::Path(path) = typed.ty.as_ref() {
                    for seg in path.path.segments.iter() {
                        if seg.ident == "GuestMemory" {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}
fn remove_guest_memory_if_exists(fn_args: Vec<FnArg>) -> Vec<FnArg> {
    let mut new_args = vec![];
    for fn_arg in fn_args {
        new_args.push(fn_arg.clone());
        match fn_arg.clone() {
            FnArg::Receiver(_) => {}
            FnArg::Typed(typed) => {
                if let Type::Path(path) = typed.ty.as_ref() {
                    for seg in path.path.segments.iter() {
                        if seg.ident == "GuestMemory" {
                            new_args.pop();
                        }
                    }
                }
            }
        }
    }
    new_args
}
fn is_async(fn_method: &ImplItemMethod) -> bool {
    fn_method.sig.asyncness.is_some()
}
fn get_fn_method(fn_item: ImplItem) -> ImplItemMethod {
    match fn_item {
        ImplItem::Method(method) => method,
        _ => panic!("there is a non-method item within this impl block"),
    }
}
