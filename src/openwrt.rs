use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
    Stack,
};
use embassy_time::{Duration, Instant, Timer};
use esp_backtrace as _;
use esp_println::println;
use esp_wifi::wifi::{WifiDevice, WifiStaDevice};
use libm::Libm;
use reqwless::{client::HttpClient, request::Method};

use crate::{
    bus::{NetDataTrafficSpeed, WiFiConnectStatus, NET_DATA_TRAFFIC_SPEED, WIFI_CONNECT_STATUS},
    openwrt_types,
};

#[embassy_executor::task]
pub async fn netdata_info(stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>) {
    let mut header_rx_buf = [0; 512];
    let mut body_rx_buf = [0; 4096];

    let mut prev_fetch_at: Instant = Instant::now();

    loop {
        let wifi_status_guard = WIFI_CONNECT_STATUS.lock().await;

        if matches!(*wifi_status_guard, WiFiConnectStatus::Connecting) {
            drop(wifi_status_guard);
            // println!("Waiting for wifi...");
            Timer::after(Duration::from_millis(1_00)).await;
            continue;
        }
        drop(wifi_status_guard);

        let tcp_client_state: TcpClientState<1, 1024, 1024> = TcpClientState::new();
        let tcp_client = TcpClient::new(stack, &tcp_client_state);
        let dns_socket = DnsSocket::new(&stack);

        let url = "http://192.168.31.1:19990/api/v1/data?after=-60&chart=net.pppoe-wan&dimensions=received|sent&format=json&group=average&gtime=0&options=absolute|jsonwrap|nonzero&points=30&timeout=100";
        let mut client = HttpClient::new(&tcp_client, &dns_socket); // Types implementing embedded-nal-async

        let mut request = client.request(Method::GET, &url).await.unwrap();
        let response = request.send(&mut header_rx_buf).await.unwrap();
        let mut reader = response.body().reader();

        let size = reader.read_to_end(&mut body_rx_buf).await.unwrap();
        let (data, _) =
            serde_json_core::de::from_slice::<'_, openwrt_types::Data>(&body_rx_buf[..size])
                .unwrap();

        let pub_msg = NetDataTrafficSpeed {
            up: Libm::<f32>::fabs(data.latest_values[1]) as u32,
            down: Libm::<f32>::fabs(data.latest_values[0]) as u32,
        };

        let mut speed = NET_DATA_TRAFFIC_SPEED.lock().await;
        *speed = pub_msg;
        drop(speed);

        let wait = prev_fetch_at.checked_add(Duration::from_secs(data.update_every as u64));
        prev_fetch_at = Instant::now();

        println!("curr: {:?}", Instant::now());
        if let Some(wait) = wait {
            Timer::at(wait).await;
        } else {
            Timer::after_millis(200).await;
        }
    }
}
