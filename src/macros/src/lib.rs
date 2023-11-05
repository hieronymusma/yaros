#![feature(proc_macro_diagnostic)]

extern crate proc_macro;

use core::panic;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::HashSet;
use syn::parse::Parse;
use syn::spanned::Spanned;
use syn::{parenthesized, parse_macro_input, FnArg, Ident, ItemExternCrate, ItemUse, Token};
use syn::{token, Type};

struct Syscalls {
    extern_imports: Vec<syn::ItemExternCrate>,
    imports: Vec<syn::ItemUse>,
    syscalls: Vec<Syscall>,
}

struct Syscall {
    id: usize,
    name: syn::Ident,
    args: Vec<syn::FnArg>,
}

impl Parse for Syscalls {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut syscalls = Vec::<Syscall>::new();
        let mut imports = Vec::<syn::ItemUse>::new();
        let mut extern_imports = Vec::<syn::ItemExternCrate>::new();
        let mut next_syscall_id = 0;

        while !input.is_empty() {
            if input.peek(token::Extern) {
                let extern_import = input.parse::<syn::ItemExternCrate>()?;
                extern_imports.push(extern_import);
                continue;
            }

            if input.peek(token::Use) {
                let import = input.parse::<syn::ItemUse>()?;
                imports.push(import);
                continue;
            }

            let syscall_name = input.parse::<Ident>()?;

            let args_content;
            parenthesized!(args_content in input);

            let args = args_content.parse_terminated(syn::FnArg::parse, Token![,])?;

            input.parse::<Token![;]>()?;

            syscalls.push(Syscall {
                id: next_syscall_id,
                name: syscall_name,
                args: args.into_iter().collect(),
            });
            next_syscall_id += 1;
        }

        Ok(Self {
            extern_imports,
            imports,
            syscalls,
        })
    }
}

/// Generates boilerplate code for the provided syscalls
/// # Example
/// ```
/// syscalls!(
///     extern crate alloc;
///
///     use alloc::vec::Vec;
///
///     WRITE_CHAR(c: u8);
///     SHARE_VEC(vec: &mut Vec<u8>, additional_data: usize);
/// )
/// ```
#[proc_macro]
pub fn syscalls(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let Syscalls {
        extern_imports,
        imports,
        syscalls,
    } = parse_macro_input!(input as Syscalls);

    match syscalls_impl(extern_imports, imports, syscalls) {
        Ok(tokens) => tokens,
        Err(_) => proc_macro::TokenStream::new(),
    }
}

fn syscalls_impl(
    extern_imports: Vec<ItemExternCrate>,
    imports: Vec<ItemUse>,
    syscalls: Vec<Syscall>,
) -> Result<proc_macro::TokenStream, ()> {
    check_for_duplicates_and_report_error(&syscalls)?;
    let userspace_syscall_functions = generate_userspace_functions(&syscalls)?;
    let userspace_module =
        generate_userspace_module(&extern_imports, &imports, userspace_syscall_functions);

    let kernel_syscall_functions = generate_kernel_functions(&syscalls)?;
    let kernel_syscall_matcharms = generate_kernel_matcharms(&syscalls)?;
    let kernel_module = generate_kernel_module(
        &extern_imports,
        &imports,
        kernel_syscall_functions,
        kernel_syscall_matcharms,
    )?;

    let all = quote! {
        #userspace_module
        #kernel_module
    };

    Ok(proc_macro::TokenStream::from(all))
}

fn generate_kernel_matcharms(syscalls: &[Syscall]) -> Result<Vec<TokenStream>, ()> {
    let mut kernel_syscall_matcharms = Vec::new();

    for syscall in syscalls {
        let syscall_nr = syscall.id;
        let syscall_name = &syscall.name;
        let syscall_arguments = &syscall.args;
        let matcharm_arguments = generate_kernel_matcharms_arguments(syscall_arguments)?;

        kernel_syscall_matcharms.push(quote! {
            #syscall_nr => Self::#syscall_name(#matcharm_arguments),
        });
    }
    Ok(kernel_syscall_matcharms)
}

fn generate_kernel_matcharms_arguments(arguments: &[FnArg]) -> Result<TokenStream, ()> {
    let mut argument_tokens = Vec::<TokenStream>::new();
    for (index, argument) in arguments.iter().enumerate() {
        let argument_type = get_argument_type(argument)?;
        let register_index = format_ident!("a{}", index);
        let argument_token = match argument_type {
            ArgumentType::Value => quote! { trap_frame[Register::#register_index] as _ },
            ArgumentType::Reference => {
                quote! { Userpointer::new(trap_frame[Register::#register_index] as _) }
            }
            ArgumentType::MutableReference => {
                quote! { UserpointerMut::new(trap_frame[Register::#register_index] as _) }
            }
        };
        argument_tokens.push(argument_token);
    }
    Ok(quote!(#(#argument_tokens),*))
}

fn generate_kernel_functions(syscalls: &[Syscall]) -> Result<Vec<TokenStream>, ()> {
    let mut kernel_functions = Vec::new();

    for syscall in syscalls {
        let syscall_name = &syscall.name;
        let syscall_arguments = change_arguments_to_userpointer(&syscall.args)?;

        kernel_functions.push(quote! {
            #[allow(non_snake_case)]
            fn #syscall_name(#(#syscall_arguments),*) -> isize;
        });
    }
    Ok(kernel_functions)
}

fn change_arguments_to_userpointer(arguments: &Vec<FnArg>) -> Result<Vec<TokenStream>, ()> {
    let mut new_arguments = Vec::new();
    for argument in arguments {
        match argument {
            FnArg::Typed(typed) => match &*typed.ty {
                Type::Reference(reference) => match &*reference.elem {
                    Type::Path(path) => {
                        let name = &typed.pat;
                        let typ = &path;
                        if reference.mutability.is_none() {
                            new_arguments.push(quote! {
                                #name: Userpointer<#typ>
                            });
                        } else {
                            new_arguments.push(quote! {
                                #name: UserpointerMut<#typ>
                            });
                        }
                    }
                    _ => {
                        argument
                            .span()
                            .unwrap()
                            .error(format!(
                                "change_arguments_to_userpointer: unsupported argument type {:?}",
                                argument
                            ))
                            .emit();
                        return Err(());
                    }
                },
                _ => new_arguments.push(quote! { #argument }),
            },
            FnArg::Receiver(s) => new_arguments.push(quote! { #s }),
        }
    }
    Ok(new_arguments)
}

fn generate_kernel_module(
    extern_imports: &Vec<ItemExternCrate>,
    imports: &Vec<ItemUse>,
    kernel_functions: Vec<TokenStream>,
    match_arms: Vec<TokenStream>,
) -> Result<TokenStream, ()> {
    Ok(quote! {
        pub mod kernel {
            #(#extern_imports)*
            #(#imports)*

            use crate::syscalls::trap_frame::TrapFrame;
            use crate::syscalls::trap_frame::Register;
            use crate::syscalls::userpointer::Userpointer;
            use crate::syscalls::userpointer::UserpointerMut;

            pub trait Syscalls {
                #(#kernel_functions)*

                fn handle(trap_frame: &mut TrapFrame) -> isize {
                    let syscall_nr = trap_frame[Register::a7];
                    match syscall_nr {
                        #(#match_arms)*
                        _ => panic!("generate_kernel_module: Unknown syscall number {}", syscall_nr),
                    }
                }
            }
        }
    })
}

fn generate_userspace_module(
    extern_imports: &Vec<ItemExternCrate>,
    imports: &Vec<ItemUse>,
    userspace_functions: Vec<TokenStream>,
) -> TokenStream {
    quote! {
        pub mod userspace {

            #(#extern_imports)*
            #(#imports)*

            use core::arch::asm;

            fn ecall_1(nr: usize, arg0: usize) -> isize {
                let ret: isize;
                unsafe {
                    asm!("ecall",
                        in("a7") nr,
                        in("a0") arg0,
                        lateout("a0") ret,
                    );
                }
                ret
            }

            fn ecall_2(nr: usize, arg0: usize, arg1: usize) -> isize {
                let ret: isize;
                unsafe {
                    asm!("ecall",
                        in("a7") nr,
                        in("a0") arg0,
                        in("a1") arg1,
                        lateout("a0") ret,
                    );
                }
                ret
            }

            #(#userspace_functions)*
        }
    }
}

fn generate_userspace_functions(syscalls: &[Syscall]) -> Result<Vec<proc_macro2::TokenStream>, ()> {
    let mut userspace_functions = Vec::new();

    for syscall in syscalls {
        let syscall_name = &syscall.name;
        let syscall_arguments = &syscall.args;
        let ecall = generate_ecall(syscall.id, syscall_arguments)?;

        userspace_functions.push(quote! {
            #[allow(non_snake_case)]
            pub fn #syscall_name(#(#syscall_arguments),*) -> isize {
                #ecall
            }
        });
    }
    Ok(userspace_functions)
}

fn generate_ecall(
    syscall_number: usize,
    arguments: &[FnArg],
) -> Result<proc_macro2::TokenStream, ()> {
    match arguments.len() {
        1 => {
            let arg0 = cast_argument(&arguments[0])?;
            Ok(quote! {
                ecall_1(#syscall_number, #arg0)
            })
        }
        2 => {
            let arg0 = cast_argument(&arguments[0])?;
            let arg1 = cast_argument(&arguments[1])?;
            Ok(quote! {
                ecall_2(#syscall_number, #arg0, #arg1)
            })
        }
        _ => panic!(
            "Number of syscall arguments ({}) not implemented yet",
            arguments.len()
        ),
    }
}

fn cast_argument(argument: &FnArg) -> Result<TokenStream, ()> {
    let argument_name = format_ident!("{}", get_argument_name(argument));
    match get_argument_type(argument)? {
        ArgumentType::Reference => Ok(quote! { #argument_name as *const _ as usize }),
        ArgumentType::MutableReference => Ok(quote! { #argument_name as *const _ as usize }),
        ArgumentType::Value => Ok(quote! { #argument_name as usize }),
    }
}

enum ArgumentType {
    Reference,
    MutableReference,
    Value,
}

fn get_argument_type(argument: &FnArg) -> Result<ArgumentType, ()> {
    let result = match argument {
        FnArg::Typed(typed) => match &*typed.ty {
            syn::Type::Reference(reference) => {
                if reference.mutability.is_some() {
                    Ok(ArgumentType::MutableReference)
                } else {
                    Ok(ArgumentType::Reference)
                }
            }
            syn::Type::Path(path) => {
                if path.path.segments.len() == 1 {
                    let segment = &path.path.segments[0];
                    if is_ident_value_type(&segment.ident) {
                        Ok(ArgumentType::Value)
                    } else {
                        Err(())
                    }
                } else {
                    Err(())
                }
            }
            _ => Err(()),
        },
        _ => Err(()),
    };

    if result.is_err() {
        argument
            .span()
            .unwrap()
            .error(format!("unsupported argument type {:?}", argument))
            .emit();
    }

    result
}

fn get_argument_name(argument: &FnArg) -> String {
    match argument {
        FnArg::Receiver(_) => "self".into(),
        FnArg::Typed(typed) => match &*typed.pat {
            syn::Pat::Ident(pat_ident) => pat_ident.ident.to_string(),
            _ => panic!("Cannot get name of argument {:?}", argument),
        },
    }
}

fn is_ident_value_type(ident: &Ident) -> bool {
    let token_stream_type = ident.to_string();
    matches!(
        token_stream_type.as_str(),
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128"
    )
}

fn check_for_duplicates_and_report_error(syscalls: &[Syscall]) -> Result<(), ()> {
    let duplicates = find_syscall_name_duplicates(syscalls);

    if duplicates.is_empty() {
        return Ok(());
    }

    for duplicate in duplicates {
        duplicate
            .span()
            .unwrap()
            .error(format!("duplicate syscall name `{}`", duplicate))
            .emit();
    }
    Err(())
}

fn find_syscall_name_duplicates(syscalls: &[Syscall]) -> Vec<&Ident> {
    let mut duplicates = Vec::<&Ident>::new();
    let mut names = HashSet::<&Ident>::new();

    for syscall in syscalls {
        if names.contains(&syscall.name) {
            duplicates.push(&syscall.name);
        } else {
            names.insert(&syscall.name);
        }
    }

    duplicates
}