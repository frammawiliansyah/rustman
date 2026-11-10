use std::collections::HashMap;

use serde_json::Value;

use crate::error::RustmanError;

/// Rows of iteration data; each row maps column name -> value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IterationDataset {
    pub rows: Vec<HashMap<String, String>>,
}

impl IterationDataset {
    /// Parses CSV with a header row. Short rows are padded with empty strings.
    pub fn from_csv_str(csv_content: &str) -> Result<Self, RustmanError> {
        let mut rdr = csv::ReaderBuilder::new()
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(csv_content.trim_start_matches('\u{feff}').as_bytes());
        let headers: Vec<String> = rdr
            .headers()
            .map_err(|e| RustmanError::SerializationError(format!("csv: {e}")))?
            .iter()
            .map(String::from)
            .collect();
        let mut rows = Vec::new();
        for rec in rdr.records() {
            let rec = rec.map_err(|e| RustmanError::SerializationError(format!("csv: {e}")))?;
            let row = headers
                .iter()
                .enumerate()
                .map(|(i, h)| (h.clone(), rec.get(i).unwrap_or("").to_string()))
                .collect();
            rows.push(row);
        }
        Ok(Self { rows })
    }

    /// Parses a JSON array of objects. Scalars are stringified; nested values are kept as JSON text.
    pub fn from_json_str(json_content: &str) -> Result<Self, RustmanError> {
        let v: Value = serde_json::from_str(json_content)?;
        let arr = v.as_array().ok_or_else(|| {
            RustmanError::SerializationError("iteration data must be a JSON array of objects".into())
        })?;
        let mut rows = Vec::with_capacity(arr.len());
        for (i, item) in arr.iter().enumerate() {
            let obj = item.as_object().ok_or_else(|| {
                RustmanError::SerializationError(format!("iteration data row {i} is not an object"))
            })?;
            rows.push(
                obj.iter()
                    .map(|(k, v)| {
                        let s = match v {
                            Value::String(s) => s.clone(),
                            Value::Null => String::new(),
                            other => other.to_string(),
                        };
                        (k.clone(), s)
                    })
                    .collect(),
            );
        }
        Ok(Self { rows })
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn get_row(&self, index: usize) -> Option<&HashMap<String, String>> {
        self.rows.get(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_basic() {
        let d = IterationDataset::from_csv_str("username,id\nalice,1\nbob,2\n").unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d.get_row(0).unwrap()["username"], "alice");
        assert_eq!(d.get_row(1).unwrap()["id"], "2");
        assert!(d.get_row(2).is_none());
    }

    #[test]
    fn csv_quotes_short_rows_and_empty() {
        let d = IterationDataset::from_csv_str("a,b\n\"x,y\"\n").unwrap();
        assert_eq!(d.rows[0]["a"], "x,y");
        assert_eq!(d.rows[0]["b"], "");
        assert!(IterationDataset::from_csv_str("a,b\n").unwrap().is_empty());
    }

    #[test]
    fn json_basic_and_scalars() {
        let d = IterationDataset::from_json_str(
            r#"[{"username":"alice","id":"1"},{"username":"bob","id":2,"ok":true,"n":null}]"#,
        )
        .unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d.rows[0]["username"], "alice");
        assert_eq!(d.rows[1]["id"], "2");
        assert_eq!(d.rows[1]["ok"], "true");
        assert_eq!(d.rows[1]["n"], "");
    }

    #[test]
    fn json_rejects_non_array_and_non_object_rows() {
        assert!(IterationDataset::from_json_str(r#"{"a":1}"#).is_err());
        assert!(IterationDataset::from_json_str("[1,2]").is_err());
        assert!(IterationDataset::from_json_str("not json").is_err());
    }
}
