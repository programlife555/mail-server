use jmap_proto::{
    error::method::MethodError,
    method::get::{GetRequest, GetResponse, RequestArguments},
    object::Object,
    types::{collection::Collection, property::Property, value::Value},
};

use crate::JMAP;

impl JMAP {
    pub async fn identity_get(
        &self,
        mut request: GetRequest<RequestArguments>,
    ) -> Result<GetResponse, MethodError> {
        let ids = request.unwrap_ids(self.config.get_max_objects)?;
        let properties = request.unwrap_properties(&[
            Property::Id,
            Property::Name,
            Property::Email,
            Property::ReplyTo,
            Property::Bcc,
            Property::TextSignature,
            Property::HtmlSignature,
            Property::MayDelete,
        ]);
        let account_id = request.account_id.document_id();
        let identity_ids = self
            .get_document_ids(account_id, Collection::Identity)
            .await?
            .unwrap_or_default();
        let ids = if let Some(ids) = ids {
            ids
        } else {
            identity_ids
                .iter()
                .take(self.config.get_max_objects)
                .map(Into::into)
                .collect::<Vec<_>>()
        };
        let mut response = GetResponse {
            account_id: request.account_id.into(),
            state: self
                .get_state(account_id, Collection::Identity)
                .await?
                .into(),
            list: Vec::with_capacity(ids.len()),
            not_found: vec![],
        };

        for id in ids {
            // Obtain the identity object
            let document_id = id.document_id();
            if !identity_ids.contains(document_id) {
                response.not_found.push(id);
                continue;
            }
            let mut push = if let Some(push) = self
                .get_property::<Object<Value>>(
                    account_id,
                    Collection::Identity,
                    document_id,
                    Property::Value,
                )
                .await?
            {
                push
            } else {
                response.not_found.push(id);
                continue;
            };
            let mut result = Object::with_capacity(properties.len());
            for property in &properties {
                match property {
                    Property::Id => {
                        result.append(Property::Id, Value::Id(id));
                    }
                    Property::MayDelete => {
                        result.append(Property::MayDelete, Value::Bool(true));
                    }
                    property => {
                        result.append(property.clone(), push.remove(property));
                    }
                }
            }
            response.list.push(result);
        }

        Ok(response)
    }
}
