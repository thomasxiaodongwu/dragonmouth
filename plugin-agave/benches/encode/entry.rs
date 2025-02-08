use {
    criterion::{black_box, BatchSize, Criterion},
    prost::Message,
    prost_types::Timestamp,
    richat_plugin_agave::protobuf::{fixtures::generate_entries, ProtobufEncoder, ProtobufMessage},
    richat_proto::plugin::{
        filter::message::{FilteredUpdate, FilteredUpdateFilters, FilteredUpdateOneof},
        message::MessageEntry,
    },
    std::{sync::Arc, time::SystemTime},
};

pub fn bench_encode_entries(criterion: &mut Criterion) {
    let entries = generate_entries();

    let entries_replica = entries.iter().map(|e| e.to_replica()).collect::<Vec<_>>();

    let entries_grpc = entries_replica
        .iter()
        .map(MessageEntry::from_geyser)
        .map(Arc::new)
        .collect::<Vec<_>>();

    criterion
        .benchmark_group("encode_entry")
        .bench_with_input("richat/prost", &entries_replica, |criterion, entries| {
            let created_at = SystemTime::now();
            criterion.iter(|| {
                #[allow(clippy::unit_arg)]
                black_box({
                    for entry in entries {
                        let message = ProtobufMessage::Entry { entry };
                        message.encode_with_timestamp(ProtobufEncoder::Prost, created_at);
                    }
                })
            });
        })
        .bench_with_input("richat/raw", &entries_replica, |criterion, entries| {
            let created_at = SystemTime::now();
            criterion.iter(|| {
                #[allow(clippy::unit_arg)]
                black_box({
                    for entry in entries {
                        let message = ProtobufMessage::Entry { entry };
                        message.encode_with_timestamp(ProtobufEncoder::Raw, created_at);
                    }
                })
            });
        })
        .bench_with_input(
            "dragons-mouth/encoding-only",
            &entries_grpc,
            |criterion, entry_messages| {
                let created_at = Timestamp::from(SystemTime::now());
                criterion.iter_batched(
                    || entry_messages.to_owned(),
                    |entry_messages| {
                        #[allow(clippy::unit_arg)]
                        black_box({
                            for message in entry_messages {
                                let update = FilteredUpdate {
                                    filters: FilteredUpdateFilters::new(),
                                    message: FilteredUpdateOneof::entry(message),
                                    created_at,
                                };
                                update.encode_to_vec();
                            }
                        })
                    },
                    BatchSize::LargeInput,
                );
            },
        )
        .bench_with_input(
            "dragons-mouth/full-pipeline",
            &entries_replica,
            |criterion, entries| {
                let created_at = Timestamp::from(SystemTime::now());
                criterion.iter(|| {
                    #[allow(clippy::unit_arg)]
                    black_box({
                        for entry in entries {
                            let message = MessageEntry::from_geyser(entry);
                            let update = FilteredUpdate {
                                filters: FilteredUpdateFilters::new(),
                                message: FilteredUpdateOneof::entry(Arc::new(message)),
                                created_at,
                            };
                            update.encode_to_vec();
                        }
                    })
                });
            },
        );
}
