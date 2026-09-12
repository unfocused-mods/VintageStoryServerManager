use super::*;
use axum::{extract::State, http::StatusCode, Json, Router};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
#[derive(Clone, Default)]
struct Fixture {
    requests: Arc<Mutex<Vec<String>>>,
    sku_response: Arc<Mutex<Option<Value>>>,
    resources: Arc<Mutex<HashMap<String, Value>>>,
    deletes: Arc<Mutex<Vec<String>>>,
    reject: Arc<Mutex<Option<String>>>,
    quota: Arc<Mutex<bool>>,
}
async fn handler(
    State(f): State<Fixture>,
    method: axum::http::Method,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> (StatusCode, Json<Value>) {
    if method == axum::http::Method::POST && body.is_empty() {
        return (if headers.get("content-length").and_then(|v| v.to_str().ok()) == Some("0") { StatusCode::OK } else { StatusCode::LENGTH_REQUIRED }, Json(json!({})));
    }
    let path = uri.path().to_string();
    f.requests.lock().unwrap().push(uri.to_string());
    if f.reject
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|s| path.contains(s))
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"fixture rejection"})),
        );
    }
    if path.ends_with("/skus") {
        // Simulate a worldwide response too large for the bounded catalog reader.
        if !uri.query().unwrap_or("").contains("$filter=location") {
            return (
                StatusCode::OK,
                Json(json!({"value":vec![json!({"name":"irrelevant"});50001]})),
            );
        }
        if let Some(value) = f.sku_response.lock().unwrap().clone() {
            return (StatusCode::OK, Json(value));
        }
        return (
            StatusCode::OK,
            Json(
                json!({"value":[{"name":"Standard_D2s_v5","resourceType":"virtualMachines","locations":["australiaeast"],"restrictions":[],"family":"standardDSv5Family","capabilities":[{"name":"CpuArchitectureType","value":"x64"},{"name":"vCPUs","value":"2"}]}]}),
            ),
        );
    }
    if path.ends_with("/usages") {
        let limit = if *f.quota.lock().unwrap() { 0 } else { 20 };
        return (
            StatusCode::OK,
            Json(
                json!({"value":[{"name":{"value":"cores"},"limit":limit,"currentValue":0},{"name":{"value":"standardDSv5Family"},"limit":limit,"currentValue":0}]}),
            ),
        );
    }
    if path.ends_with("/providers/Microsoft.Compute")
        || path.ends_with("/providers/Microsoft.Network")
    {
        return (
            StatusCode::OK,
            Json(json!({"registrationState":"Registered"})),
        );
    }
    if path == "/subscriptions/00000000-0000-0000-0000-000000000001" {
        return (StatusCode::OK, Json(json!({"state":"Enabled"})));
    }
    let mut resources = f.resources.lock().unwrap();
    match method {
        axum::http::Method::GET => match resources.get(&path) {
            Some(v) => (StatusCode::OK, Json(v.clone())),
            None => (StatusCode::NOT_FOUND, Json(json!({}))),
        },
        axum::http::Method::PUT => {
            let mut value: Value = serde_json::from_slice(&body).unwrap();
            value["properties"]["provisioningState"] = json!("Succeeded");
            if path.contains("/publicIPAddresses/") {
                value["properties"]["ipAddress"] = json!("192.0.2.10");
            }
            if path.contains("/virtualMachines/") {
                let disk_path = format!(
                    "{}/disks/{}",
                    path.split("/virtualMachines/").next().unwrap(),
                    value["properties"]["storageProfile"]["osDisk"]["name"]
                        .as_str()
                        .unwrap()
                );
                resources.insert(
                    disk_path,
                    json!({"properties":{"provisioningState":"Succeeded"}}),
                );
            }
            resources.insert(path, value.clone());
            (StatusCode::OK, Json(value))
        }
        axum::http::Method::PATCH => {
            let input: Value = serde_json::from_slice(&body).unwrap();
            let Some(value) = resources.get_mut(&path) else {
                return (StatusCode::NOT_FOUND, Json(json!({})));
            };
            if input["tags"].is_object() {
                value["tags"] = input["tags"].clone();
            }
            (StatusCode::OK, Json(value.clone()))
        }
        axum::http::Method::DELETE => {
            f.deletes.lock().unwrap().push(path.clone());
            resources.remove(&path);
            (StatusCode::OK, Json(json!({})))
        }
        _ => (StatusCode::OK, Json(json!({}))),
    }
}
async fn setup() -> (Cloud, Fixture, tokio::task::JoinHandle<()>) {
    let f = Fixture::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().fallback(handler).with_state(f.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut cloud = Cloud::new(Account {
        id: "azure".into(),
        name: "fixture".into(),
        provider: Provider::Azure,
        credential_ref: "".into(),
        settings: json!({"subscription_id":"00000000-0000-0000-0000-000000000001"}),
    });
    cloud.endpoint = Some(url);
    (cloud, f, task)
}
#[tokio::test]
async fn azure_discovery_and_recovery_preserve_ownership_and_network_order() {
    let (cloud,f,task)=setup().await;
    let group="/subscriptions/00000000-0000-0000-0000-000000000001/resourceGroups/recover";
    let owner="11111111-1111-4111-8111-111111111111";
    let vmid=format!("{group}/providers/Microsoft.Compute/virtualMachines/vsm");
    let nic=format!("{group}/providers/Microsoft.Network/networkInterfaces/vsm");
    let ip=format!("{group}/providers/Microsoft.Network/publicIPAddresses/vsm");
    let vm=json!({"id":vmid,"type":"Microsoft.Compute/virtualMachines","tags":{"vsm-owner":owner},"location":"newzealandnorth","properties":{"hardwareProfile":{"vmSize":"Standard_D2s_v5"},"osProfile":{"adminUsername":"vsm"},"networkProfile":{"networkInterfaces":[{"id":nic}]}}});
    let mut uppercase=vm.clone(); uppercase["id"]=json!(vmid.to_ascii_uppercase());
    assert_eq!(discovery_server(&uppercase,&cloud.account).unwrap().id,owner);
    let network=|id:&str,ty:&str|json!({"id":id,"type":ty,"tags":{"vsm-owner":owner}});
    {
        let mut r=f.resources.lock().unwrap();
        r.insert("/subscriptions/00000000-0000-0000-0000-000000000001/providers/Microsoft.Compute/virtualMachines".into(),json!({"value":[vm.clone(),{"id":"unowned"}]}));
        r.insert(vmid.clone(),vm.clone());
        r.insert(format!("{group}/resources"),json!({"value":[network(&nic,"Microsoft.Network/networkInterfaces"),vm,network(&ip,"Microsoft.Network/publicIPAddresses"),{"id":"unowned","type":"Microsoft.Compute/disks"}]}));
        r.insert(nic.clone(),json!({"properties":{"ipConfigurations":[{"properties":{"publicIPAddress":{"id":ip}}}]}}));
        r.insert(ip.clone(),json!({"properties":{"ipAddress":"203.0.113.10"}}));
    }
    let list=cloud.discover_azure().await.unwrap();
    assert_eq!(list["deployments"].as_array().unwrap().len(),1);
    let server=cloud.recover_azure(&vmid).await.unwrap();
    assert_eq!(server.id,owner); assert_eq!(server.state,"recovered"); assert!(server.game_version.is_empty());
    assert_eq!(server.host,"203.0.113.10"); assert_eq!(server.resources.len(),3);
    assert_eq!(server.resources[0].id,ip); assert_eq!(server.resources[1].id,nic);
    assert!(f.deletes.lock().unwrap().is_empty());
    f.resources.lock().unwrap().get_mut(&format!("{group}/resources")).unwrap()["value"].as_array_mut().unwrap().push(network(&format!("{group}/providers/Microsoft.Compute/virtualMachines/other"),"Microsoft.Compute/virtualMachines"));
    assert!(cloud.recover_azure(&vmid).await.unwrap_err().to_string().contains("Multiple VMs"));
    task.abort();
}
#[tokio::test]
async fn azure_discovery_rejects_hostile_continuations_and_partial_reads() {
    let (cloud,f,task)=setup().await;
    let path="/subscriptions/00000000-0000-0000-0000-000000000001/providers/Microsoft.Compute/virtualMachines";
    f.resources.lock().unwrap().insert(path.into(),json!({"value":[],"nextLink":"https://evil.example/steal"}));
    assert!(cloud.discover_azure().await.unwrap_err().to_string().contains("Unsafe"));
    f.resources.lock().unwrap().insert(path.into(),json!({"value":[],"nextLink":format!("{}{path}",cloud.base())}));
    assert!(cloud.discover_azure().await.unwrap_err().to_string().contains("repeated"));
    *f.reject.lock().unwrap()=Some("virtualMachines".into());
    assert!(cloud.discover_azure().await.unwrap_err().to_string().contains("403"));
    task.abort();
}
#[tokio::test]
async fn regional_catalog_is_filtered_cached_and_account_scoped() {
    let (mut cloud, f, task) = setup().await;
    f.resources.lock().unwrap().insert(
        "/subscriptions/00000000-0000-0000-0000-000000000001/locations".into(),
        json!({"value":[{"name":"australiaeast"}]}),
    );
    assert_eq!(
        cloud.regions().await.unwrap()["regions"][0]["name"],
        "australiaeast"
    );
    assert_eq!(
        f.requests.lock().unwrap().len(),
        1,
        "regions must not wait for SKU discovery"
    );
    let first = cloud.sizes("australiaeast").await.unwrap();
    assert_eq!(first["sizes"].as_array().unwrap().len(), 1);
    assert!(f
        .requests
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .contains("$filter=location%20eq%20%27australiaeast%27"));
    assert_eq!(cloud.sizes("australiaeast").await.unwrap(), first);
    assert_eq!(
        f.requests.lock().unwrap().len(),
        2,
        "cached region must avoid another ARM request"
    );
    cloud.account.credential_ref = "changed-reference".into();
    cloud.sizes("australiaeast").await.unwrap();
    assert_eq!(
        f.requests.lock().unwrap().len(),
        3,
        "credential edits invalidate cache"
    );
    invalidate_catalog(&cloud.account.id).await;
    cloud.sizes("australiaeast").await.unwrap();
    assert_eq!(
        f.requests.lock().unwrap().len(),
        4,
        "account deletion/recreation must invalidate cached metadata"
    );
    assert!(cloud.sizes("australiaeast' or true").await.is_err());
    task.abort();
}
#[tokio::test]
async fn catalog_rejects_malformed_pages_and_untrusted_continuations() {
    let (cloud, f, task) = setup().await;
    for response in [
        json!({"value":null}),
        json!({"value":[],"nextLink":23}),
        json!({"value":[],"nextLink":"https://management.azure.com.evil.test/stolen"}),
        json!({"value":[],"nextLink":"https://user@management.azure.com/next"}),
    ] {
        *f.sku_response.lock().unwrap() = Some(response);
        assert!(cloud
            .azure_pages(&regional_skus_path(
                cloud.subscription().unwrap(),
                "australiaeast"
            ))
            .await
            .is_err());
    }
    task.abort();
}
fn spec() -> DeploySpec {
    DeploySpec {
        name: "fixture-world".into(),
        account_id: "azure".into(),
        region: "australiaeast".into(),
        size: "Standard_D2s_v5".into(),
        game_version: "1.22.0".into(),
        ssh_public_key: "ssh-ed25519 AAAATEST".into(),
        ssh_private_key: "private-path".into(),
        admin_cidr: "192.0.2.1/32".into(),
        existing_host: "".into(),
        ssh_user: "vsm".into(),
    }
}
fn server() -> Server {
    Server {
        id: uuid::Uuid::new_v4().to_string(),
        name: "fixture-world".into(),
        account_id: Some("azure".into()),
        provider: Provider::Azure,
        region: "australiaeast".into(),
        size: "Standard_D2s_v5".into(),
        host: "".into(),
        ssh_user: "vsm".into(),
        ssh_key: "private-path".into(),
        game_version: "1.22.0".into(),
        state: "provisioning".into(),
        created_at: now(),
        resources: vec![],
    }
}
#[tokio::test]
async fn azure_provisions_and_deletes_only_owned_individual_resources() {
    let (c, f, task) = setup().await;
    let t = tempfile::tempdir().unwrap();
    let db = Db::open(&t.path().join("db")).unwrap();
    let mut s = server();
    c.provision(&spec(), &mut s, &db).await.unwrap();
    assert_eq!(s.host, "192.0.2.10");
    assert_eq!(s.resources.len(), 7);
    let nsg = s
        .resources
        .iter()
        .find(|r| r.id.contains("networkSecurityGroups"))
        .unwrap();
    assert_eq!(
        f.resources.lock().unwrap()[&nsg.id]["properties"]["securityRules"][0]["properties"]
            ["sourceAddressPrefix"],
        "192.0.2.1/32"
    );
    c.delete(&s).await.unwrap();
    assert!(!f
        .deletes
        .lock()
        .unwrap()
        .iter()
        .any(|p| !p.contains("/providers/")));
    assert_eq!(f.resources.lock().unwrap().len(), 1);
    // A new cleanup attempt after interruption skips resources already removed.
    c.delete(&s).await.unwrap();
    assert_eq!(f.resources.lock().unwrap().len(), 1);
    task.abort();
}
#[tokio::test]
async fn azure_quota_failure_creates_nothing() {
    let (c, f, task) = setup().await;
    *f.quota.lock().unwrap() = true;
    let t = tempfile::tempdir().unwrap();
    let db = Db::open(&t.path().join("db")).unwrap();
    assert!(c.provision(&spec(), &mut server(), &db).await.is_err());
    assert!(f.resources.lock().unwrap().is_empty());
    task.abort();
}
#[tokio::test]
async fn azure_partial_failure_keeps_resource_journal() {
    let (c, f, task) = setup().await;
    *f.reject.lock().unwrap() = Some("networkInterfaces".into());
    let t = tempfile::tempdir().unwrap();
    let db = Db::open(&t.path().join("db")).unwrap();
    let mut s = server();
    assert!(c.provision(&spec(), &mut s, &db).await.is_err());
    assert!(!f.resources.lock().unwrap().is_empty());
    assert!(db
        .server(&s.id)
        .unwrap()
        .resources
        .iter()
        .any(|r| r.id.contains("networkInterfaces")));
    task.abort();
}
#[tokio::test]
async fn azure_ownership_mismatch_prevents_mutation() {
    let (c, f, task) = setup().await;
    let t = tempfile::tempdir().unwrap();
    let db = Db::open(&t.path().join("db")).unwrap();
    let mut s = server();
    c.provision(&spec(), &mut s, &db).await.unwrap();
    let vm = s.resources.iter().find(|r| r.kind == "vm").unwrap();
    f.resources.lock().unwrap().get_mut(&vm.id).unwrap()["tags"]["vsm-owner"] =
        json!("someone-else");
    assert!(c.delete(&s).await.is_err());
    assert!(f.deletes.lock().unwrap().is_empty());
    task.abort();
}

#[tokio::test]
async fn new_zealand_region_case_and_restrictions_survive_fresh_preflight() {
    let (cloud, f, task) = setup().await;
    let sku = json!({"name":"Standard_D2s_v5","resourceType":"virtualMachines","locations":["NewZealandNorth"],"restrictions":[],"family":"standardDSv5Family","capabilities":[{"name":"CpuArchitectureType","value":"x64"},{"name":"vCPUs","value":"2"}]});
    *f.sku_response.lock().unwrap() = Some(json!({"value":[sku.clone()]}));
    assert_eq!(
        cloud.sizes("newzealandnorth").await.unwrap()["sizes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut request = spec();
    request.region = "newzealandnorth".into();
    cloud.azure_preflight(&request).await.unwrap();
    assert!(f
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|p| p.contains("/skus?"))
        .all(|p| p.contains("$filter=location%20eq%20%27newzealandnorth%27")));
    let mut restricted = sku.clone();
    restricted["restrictions"] =
        json!([{"type":"Zone","values":["NewZealandNorth"],"restrictionInfo":{"zones":["1"]}}]);
    assert!(!sku_region_restricted(&restricted, "newzealandnorth"));
    restricted["restrictions"] =
        json!([{"type":"Location","restrictionInfo":{"locations":["NewZealandNorth"]}}]);
    assert!(sku_region_restricted(&restricted, "newzealandnorth"));
    restricted["restrictions"] = json!([{"type":"Location","values":["AustraliaEast"]}]);
    assert!(!sku_region_restricted(&restricted, "newzealandnorth"));
    task.abort();
}
#[tokio::test]
#[ignore = "Read-only signed-in Azure account; requires VSM_TEST_SUBSCRIPTION"]
async fn live_new_zealand_catalog_and_quote() {
    let subscription = std::env::var("VSM_TEST_SUBSCRIPTION").unwrap();
    let cloud = Cloud::new(Account {
        id: uuid::Uuid::new_v4().to_string(),
        name: "read-only catalog verification".into(),
        provider: Provider::Azure,
        credential_ref: "".into(),
        settings: json!({"subscription_id":subscription,"auth":"azure_cli"}),
    });
    let start = std::time::Instant::now();
    let catalog = cloud.sizes("newzealandnorth").await.unwrap();
    println!(
        "NZ North catalog: {} VM sizes in {:?}",
        catalog["sizes"].as_array().unwrap().len(),
        start.elapsed()
    );
    assert!(!catalog["sizes"].as_array().unwrap().is_empty());
    let start = std::time::Instant::now();
    let quote = cloud
        .quote("NewZealandNorth", "Standard_D2s_v5")
        .await
        .unwrap();
    println!(
        "Fresh NZ North quote: available={}, compute={} {}, elapsed={:?}",
        quote.available,
        quote.monthly_compute,
        quote.currency,
        start.elapsed()
    );
    assert!(quote.available);
    assert_eq!(quote.vcpus, 2);
    assert_eq!(quote.memory_gib, 8.0);
}

#[tokio::test]
async fn empty_power_posts_send_explicit_length_and_status_reads_only_vm() {
    let (cloud, f, task) = setup().await;
    cloud.request(Method::POST, "/vm/start", None).await.unwrap();
    let mut vm = server();
    vm.resources.push(Resource { id:"/vm".into(), kind:"vm".into(), owned:true });
    f.resources.lock().unwrap().insert("/vm/instanceView".into(), json!({"statuses":[{"code":"PowerState/running"},{"code":"ProvisioningState/succeeded"}]}));
    let status = cloud.infrastructure_status(&vm).await.unwrap();
    assert_eq!(status["power_state"], "running");
    assert_eq!(status["provisioning_state"], "succeeded");
    assert_eq!(f.requests.lock().unwrap().as_slice(), ["/vm/instanceView?api-version=2024-03-01"]);
    task.abort();
}
