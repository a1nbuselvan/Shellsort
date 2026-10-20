use super::model::{Category, EngineError, EngineErrorKind, RelativePath};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryRule {
    pub enabled: bool,
    /// Relative directory beneath the selected organization root.
    /// `None` explicitly leaves matching files in place; an empty string is invalid.
    pub destination: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownFileTypeBehavior {
    RouteToOther,
    KeepInPlace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationSettings {
    pub categories: BTreeMap<Category, CategoryRule>,
    pub unknown_file_types: UnknownFileTypeBehavior,
}

impl Default for OrganizationSettings {
    fn default() -> Self {
        let categories = Category::all()
            .into_iter()
            .map(|category| {
                (
                    category,
                    CategoryRule {
                        enabled: true,
                        destination: Some(category.folder_name().to_owned()),
                    },
                )
            })
            .collect();
        Self {
            categories,
            unknown_file_types: UnknownFileTypeBehavior::RouteToOther,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedCategoryRule {
    pub enabled: bool,
    pub destination: Option<RelativePath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedOrganizationSettings {
    pub categories: BTreeMap<Category, ValidatedCategoryRule>,
    pub unknown_file_types: UnknownFileTypeBehavior,
}

impl OrganizationSettings {
    pub(crate) fn validate(&self) -> Result<ValidatedOrganizationSettings, EngineError> {
        if self.categories.len() != Category::all().len() {
            return Err(invalid_settings(
                "Settings must include exactly one rule for every supported category.",
            ));
        }

        let mut categories = BTreeMap::new();
        for category in Category::all() {
            let rule = self.categories.get(&category).ok_or_else(|| {
                invalid_settings(format!(
                    "A destination rule is missing for {}.",
                    category.folder_name()
                ))
            })?;
            let destination = rule
                .destination
                .as_deref()
                .map(validate_destination)
                .transpose()
                .map_err(|_| {
                    invalid_settings(format!(
                        "The {} destination must be a non-empty relative folder path without '.', '..', a leading slash, or empty path segments. Choose ‘No destination’ to leave files in place.",
                        category.folder_name()
                    ))
                })?;
            categories.insert(
                category,
                ValidatedCategoryRule {
                    enabled: rule.enabled,
                    destination,
                },
            );
        }

        Ok(ValidatedOrganizationSettings {
            categories,
            unknown_file_types: self.unknown_file_types,
        })
    }
}

fn validate_destination(value: &str) -> Result<RelativePath, EngineError> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains('\\')
        || value.contains('\0')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(invalid_settings(
            "Destination is not a safe relative folder path.",
        ));
    }

    let path = Path::new(value);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid_settings(
            "Destination must stay inside the selected folder.",
        ));
    }
    RelativePath::from_path(path)
        .map_err(|_| invalid_settings("Destination is not a valid relative path."))
}

fn invalid_settings(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorKind::InvalidSettings, message)
}

#[cfg(test)]
mod tests {
    use super::{OrganizationSettings, UnknownFileTypeBehavior};
    use crate::organizer::{Category, EngineErrorKind};

    #[test]
    fn defaults_enable_supported_categories_and_route_unknown_types_to_other() {
        let settings = OrganizationSettings::default();
        assert_eq!(settings.categories.len(), Category::all().len());
        for category in Category::all() {
            let rule = settings.categories.get(&category).expect("default rule");
            assert!(rule.enabled);
            assert_eq!(rule.destination.as_deref(), Some(category.folder_name()));
        }
        assert_eq!(
            settings.unknown_file_types,
            UnknownFileTypeBehavior::RouteToOther
        );
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn validation_rejects_empty_absolute_and_traversing_destinations() {
        for destination in [
            "",
            "  ",
            "/outside",
            "../outside",
            "safe/../outside",
            "a//b",
            "./folder",
            "folder/",
            "folder\\child",
            "bad\0name",
        ] {
            let mut settings = OrganizationSettings::default();
            settings
                .categories
                .get_mut(&Category::Documents)
                .expect("documents rule")
                .destination = Some(destination.to_owned());
            let error = settings
                .validate()
                .expect_err("unsafe destination rejected");
            assert_eq!(
                error.kind,
                EngineErrorKind::InvalidSettings,
                "{destination:?}"
            );
        }
    }

    #[test]
    fn validation_accepts_nested_unicode_and_spaced_relative_destinations() {
        let mut settings = OrganizationSettings::default();
        settings
            .categories
            .get_mut(&Category::Documents)
            .expect("documents rule")
            .destination = Some("Archive/Études & Notes".to_owned());
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn validation_rejects_missing_category_rules() {
        let mut missing = OrganizationSettings::default();
        missing.categories.remove(&Category::Documents);
        assert_eq!(
            missing.validate().expect_err("missing rule").kind,
            EngineErrorKind::InvalidSettings
        );
    }
}
