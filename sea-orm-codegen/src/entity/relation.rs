use heck::{ToSnakeCase, ToUpperCamelCase};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use sea_query::{ForeignKeyAction, TableForeignKey};
use syn::{punctuated::Punctuated, token::Comma};

use crate::util::escape_rust_keyword;

#[derive(Clone, Debug)]
pub enum RelationType {
    HasOne,
    HasMany,
    BelongsTo,
}

#[derive(Clone, Debug)]
pub struct Relation {
    pub(crate) ref_table: String,
    pub(crate) columns: Vec<String>,
    pub(crate) ref_columns: Vec<String>,
    pub(crate) rel_type: RelationType,
    pub(crate) on_update: Option<ForeignKeyAction>,
    pub(crate) on_delete: Option<ForeignKeyAction>,
    pub(crate) self_referencing: bool,
    pub(crate) num_suffix: usize,
    pub(crate) impl_related: bool,
}

impl Relation {
    pub fn get_enum_name(&self) -> Ident {
        let name = if self.self_referencing {
            format_ident!("SelfRef")
        } else {
            format_ident!("{}", self.ref_table.to_upper_camel_case())
        };
        if self.num_suffix > 0 {
            format_ident!("{}{}", name, self.num_suffix)
        } else {
            name
        }
    }

    pub fn get_module_name(&self) -> Option<Ident> {
        if self.self_referencing {
            None
        } else {
            Some(format_ident!(
                "{}",
                escape_rust_keyword(self.ref_table.to_snake_case())
            ))
        }
    }

    pub fn get_def(&self) -> TokenStream {
        let rel_type = self.get_rel_type();
        let module_name = self.get_module_name();
        let ref_entity = if module_name.is_some() {
            quote! { super::#module_name::Entity }
        } else {
            quote! { Entity }
        };
        match self.rel_type {
            RelationType::HasOne | RelationType::HasMany => {
                quote! {
                    Entity::#rel_type(#ref_entity).into()
                }
            }
            RelationType::BelongsTo => {
                let map_src_column = |src_column: &Ident| {
                    quote! { Column::#src_column }
                };
                let map_ref_column = |ref_column: &Ident| {
                    if module_name.is_some() {
                        quote! { super::#module_name::Column::#ref_column }
                    } else {
                        quote! { Column::#ref_column }
                    }
                };
                let map_punctuated =
                    |punctuated: Punctuated<TokenStream, Comma>| match punctuated.len() {
                        0..=1 => quote! { #punctuated },
                        _ => quote! { (#punctuated) },
                    };
                let (from, to) =
                    self.get_src_ref_columns(map_src_column, map_ref_column, map_punctuated);
                quote! {
                    Entity::#rel_type(#ref_entity)
                        .from(#from)
                        .to(#to)
                        .into()
                }
            }
        }
    }

    pub fn get_attrs(&self) -> TokenStream {
        let rel_type = self.get_rel_type();
        let module_name = if let Some(module_name) = self.get_module_name() {
            format!("super::{module_name}::")
        } else {
            String::new()
        };
        let ref_entity = format!("{module_name}Entity");
        match self.rel_type {
            RelationType::HasOne | RelationType::HasMany => {
                quote! {
                    #[sea_orm(#rel_type = #ref_entity)]
                }
            }
            RelationType::BelongsTo => {
                let map_src_column = |src_column: &Ident| format!("Column::{src_column}");
                let map_ref_column =
                    |ref_column: &Ident| format!("{module_name}Column::{ref_column}");
                let map_punctuated = |punctuated: Vec<String>| {
                    let len = punctuated.len();
                    let punctuated = punctuated.join(", ");
                    match len {
                        0..=1 => punctuated,
                        _ => format!("({punctuated})"),
                    }
                };
                let (from, to) =
                    self.get_src_ref_columns(map_src_column, map_ref_column, map_punctuated);

                let on_update = if let Some(action) = &self.on_update {
                    let action = Self::get_foreign_key_action(action);
                    quote! {
                        on_update = #action,
                    }
                } else {
                    quote! {}
                };
                let on_delete = if let Some(action) = &self.on_delete {
                    let action = Self::get_foreign_key_action(action);
                    quote! {
                        on_delete = #action,
                    }
                } else {
                    quote! {}
                };
                quote! {
                    #[sea_orm(
                        #rel_type = #ref_entity,
                        from = #from,
                        to = #to,
                        #on_update
                        #on_delete
                    )]
                }
            }
        }
    }

    pub fn get_rel_type(&self) -> Ident {
        match self.rel_type {
            RelationType::HasOne => format_ident!("has_one"),
            RelationType::HasMany => format_ident!("has_many"),
            RelationType::BelongsTo => format_ident!("belongs_to"),
        }
    }

    pub fn get_column_camel_case(&self) -> Vec<Ident> {
        self.columns
            .iter()
            .map(|col| format_ident!("{}", col.to_upper_camel_case()))
            .collect()
    }

    pub fn get_ref_column_camel_case(&self) -> Vec<Ident> {
        self.ref_columns
            .iter()
            .map(|col| format_ident!("{}", col.to_upper_camel_case()))
            .collect()
    }

    pub fn get_foreign_key_action(action: &ForeignKeyAction) -> String {
        action.variant_name().to_owned()
    }

    pub fn get_src_ref_columns<F1, F2, F3, T, I>(
        &self,
        map_src_column: F1,
        map_ref_column: F2,
        map_punctuated: F3,
    ) -> (T, T)
    where
        F1: Fn(&Ident) -> T,
        F2: Fn(&Ident) -> T,
        F3: Fn(I) -> T,
        I: Extend<T> + Default,
    {
        let from: I =
            self.get_column_camel_case()
                .iter()
                .fold(I::default(), |mut acc, src_column| {
                    acc.extend([map_src_column(src_column)]);
                    acc
                });
        let to: I =
            self.get_ref_column_camel_case()
                .iter()
                .fold(I::default(), |mut acc, ref_column| {
                    acc.extend([map_ref_column(ref_column)]);
                    acc
                });

        (map_punctuated(from), map_punctuated(to))
    }
}

impl From<&TableForeignKey> for Relation {
    fn from(tbl_fk: &TableForeignKey) -> Self {
        let ref_table = match tbl_fk.get_ref_table() {
            Some(s) => s.sea_orm_table().to_string(),
            None => panic!("RefTable should not be empty"),
        };
        let columns = tbl_fk.get_columns();
        let ref_columns = tbl_fk.get_ref_columns();
        let rel_type = RelationType::BelongsTo;
        let on_delete = tbl_fk.get_on_delete();
        let on_update = tbl_fk.get_on_update();
        Self {
            ref_table,
            columns,
            ref_columns,
            rel_type,
            on_delete,
            on_update,
            self_referencing: false,
            num_suffix: 0,
            impl_related: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Relation, RelationType};
    use proc_macro2::TokenStream;
    use sea_query::ForeignKeyAction;

    fn setup() -> Vec<Relation> {
        vec![
            Relation {
                ref_table: "fruit".to_owned(),
                columns: vec!["id".to_owned()],
                ref_columns: vec!["cake_id".to_owned()],
                rel_type: RelationType::HasOne,
                on_delete: None,
                on_update: None,
                self_referencing: false,
                num_suffix: 0,
                impl_related: true,
            },
            Relation {
                ref_table: "filling".to_owned(),
                columns: vec!["filling_id".to_owned()],
                ref_columns: vec!["id".to_owned()],
                rel_type: RelationType::BelongsTo,
                on_delete: Some(ForeignKeyAction::Cascade),
                on_update: Some(ForeignKeyAction::Cascade),
                self_referencing: false,
                num_suffix: 0,
                impl_related: true,
            },
            Relation {
                ref_table: "filling".to_owned(),
                columns: vec!["filling_id".to_owned()],
                ref_columns: vec!["id".to_owned()],
                rel_type: RelationType::HasMany,
                on_delete: Some(ForeignKeyAction::Cascade),
                on_update: None,
                self_referencing: false,
                num_suffix: 0,
                impl_related: true,
            },
        ]
    }

    #[test]
    fn test_get_module_name() {
        let relations = setup();
        let snake_cases = vec!["fruit", "filling", "filling"];
        for (rel, snake_case) in relations.into_iter().zip(snake_cases) {
            assert_eq!(rel.get_module_name().unwrap().to_string(), snake_case);
        }
    }

    #[test]
    fn test_get_enum_name() {
        let relations = setup();
        let camel_cases = vec!["Fruit", "Filling", "Filling"];
        for (rel, camel_case) in relations.into_iter().zip(camel_cases) {
            assert_eq!(rel.get_enum_name().to_string(), camel_case);
        }
    }

    #[test]
    fn test_get_def() {
        let relations = setup();
        let rel_defs = vec![
            "Entity::has_one(super::fruit::Entity).into()",
            "Entity::belongs_to(super::filling::Entity) \
                .from(Column::FillingId) \
                .to(super::filling::Column::Id) \
                .into()",
            "Entity::has_many(super::filling::Entity).into()",
        ];
        for (rel, rel_def) in relations.into_iter().zip(rel_defs) {
            let rel_def: TokenStream = rel_def.parse().unwrap();

            assert_eq!(rel.get_def().to_string(), rel_def.to_string());
        }
    }

    #[test]
    fn test_get_rel_type() {
        let relations = setup();
        let rel_types = vec!["has_one", "belongs_to", "has_many"];
        for (rel, rel_type) in relations.into_iter().zip(rel_types) {
            assert_eq!(rel.get_rel_type(), rel_type);
        }
    }

    #[test]
    fn test_get_column_camel_case() {
        let relations = setup();
        let cols = vec!["Id", "FillingId", "FillingId"];
        for (rel, col) in relations.into_iter().zip(cols) {
            assert_eq!(rel.get_column_camel_case(), [col]);
        }
    }

    #[test]
    fn test_get_ref_column_camel_case() {
        let relations = setup();
        let ref_cols = vec!["CakeId", "Id", "Id"];
        for (rel, ref_col) in relations.into_iter().zip(ref_cols) {
            assert_eq!(rel.get_ref_column_camel_case(), [ref_col]);
        }
    }
}
