// Copyright 2019 Fredrik Portström <https://portstrom.com>
// This is free software distributed under the terms specified in
// the file LICENSE at the top-level directory of this distribution.

pub fn parse_end_tag(
	state: &mut crate::State,
	configuration: &crate::Configuration,
) {
	let start_position = state.scan_position;
	let tag_name_start_position = start_position + 2;
	let mut tag_name_end_position = tag_name_start_position;
	while let Some(character) = state.get_byte(tag_name_end_position) {
		match character {
			b'\t' | b'\n' | b' ' | b'/' | b'>' => break,
			b'<' => {
				state.scan_position += 1;
				return;
			}
			_ => tag_name_end_position += 1,
		}
	}
	let tag_name =
		&state.wiki_text[tag_name_start_position..tag_name_end_position];
	let tag_name = if tag_name.as_bytes().iter().all(u8::is_ascii_lowercase) {
		crate::Cow::Borrowed(tag_name)
	} else {
		tag_name.to_ascii_lowercase().into()
	};
	match configuration.tag_name_map.get(&tag_name as &str) {
		None => {
			state.scan_position = tag_name_start_position;
			state.warnings.push(crate::Warning {
				end: tag_name_end_position,
				message: crate::WarningMessage::UnrecognizedTagName,
				start: tag_name_start_position,
			});
		}
		Some(crate::TagClass::ExtensionTag) => {
			let mut tag_end_position = tag_name_end_position;
			loop {
				match state.get_byte(tag_end_position) {
					Some(b'>') => break,
					Some(b'\t') | Some(b'\n') | Some(b' ') => {
						tag_end_position += 1
					}
					_ => {
						state.scan_position = tag_name_start_position;
						state.warnings.push(crate::Warning {
							end: tag_end_position,
							message: crate::WarningMessage::InvalidTagSyntax,
							start: start_position,
						});
						return;
					}
				}
			}
			let mut matched_node_index = None;
			for (open_node_index, open_node) in
				state.stack.iter().enumerate().rev()
			{
				if let crate::OpenNodeType::Tag { name, .. } = &open_node.type_
				{
					if name == &tag_name {
						matched_node_index = Some(open_node_index);
						break;
					}
				}
			}
			match matched_node_index {
				None => {
					state.scan_position = tag_name_start_position;
					state.warnings.push(crate::Warning {
						end: tag_name_end_position,
						message: crate::WarningMessage::UnexpectedEndTag,
						start: tag_name_start_position,
					});
				}
				Some(open_node_index) => {
					if open_node_index < state.stack.len() - 1 {
						state.warnings.push(crate::Warning {
							end: tag_end_position,
							message:
								crate::WarningMessage::MissingEndTagRewinding,
							start: start_position,
						});
						state.stack.truncate(open_node_index + 2);
						let open_node = state.stack.pop().unwrap();
						state.rewind(open_node.nodes, open_node.start);
					} else {
						state.flush(start_position);
						let open_node = state.stack.pop().unwrap();
						tag_end_position += 1;
						state.flushed_position = tag_end_position;
						state.scan_position = state.flushed_position;
						let nodes = std::mem::replace(
							&mut state.nodes,
							open_node.nodes,
						);
						let content_start = match open_node.type_ {
							crate::OpenNodeType::Tag {
								content_start, ..
							} => content_start,
							_ => unreachable!("matched as a tag"),
						};
						state.nodes.push(crate::Node::Tag {
							content_end: start_position,
							content_start,
							end: state.scan_position,
							name: tag_name,
							nodes,
							start: open_node.start,
						});
					}
				}
			}
		}
		Some(crate::TagClass::Tag) => {
			let mut tag_end_position = tag_name_end_position;
			loop {
				match state.get_byte(tag_end_position) {
					None => {
						state.scan_position = tag_name_start_position;
						state.warnings.push(crate::Warning {
							end: tag_name_end_position,
							message: crate::WarningMessage::InvalidTagSyntax,
							start: tag_name_start_position,
						});
						return;
					}
					Some(b'>') => break,
					_ => tag_end_position += 1,
				}
			}
			state.flush(start_position);
			state.flushed_position = tag_end_position + 1;
			state.scan_position = state.flushed_position;
			state.nodes.push(crate::Node::EndTag {
				end: state.scan_position,
				name: tag_name,
				start: start_position,
			});
		}
	}
}

pub fn parse_start_tag(
	state: &mut crate::State,
	configuration: &crate::Configuration,
) {
	let start_position = state.scan_position;
	let tag_name_start_position = start_position + 1;
	let tag_name_end_position = match state.wiki_text.as_bytes()
		[tag_name_start_position..]
		.iter()
		.cloned()
		.position(|character| match character {
			b'\t' | b'\n' | b' ' | b'/' | b'>' => true,
			_ => false,
		}) {
		None => state.wiki_text.len(),
		Some(position) => tag_name_start_position + position,
	};
	let tag_name =
		&state.wiki_text[tag_name_start_position..tag_name_end_position];
	let tag_name = if tag_name.as_bytes().iter().all(u8::is_ascii_lowercase) {
		crate::Cow::Borrowed(tag_name)
	} else {
		tag_name.to_ascii_lowercase().into()
	};
	match configuration.tag_name_map.get(&tag_name as &str) {
		None => {
			state.scan_position = tag_name_start_position;
			state.warnings.push(crate::Warning {
				end: tag_name_end_position,
				message: crate::WarningMessage::UnrecognizedTagName,
				start: tag_name_start_position,
			});
		}
		Some(tag_class) => match state.wiki_text.as_bytes()
			[tag_name_end_position..]
			.iter()
			.cloned()
			.position(|character| character == b'>')
		{
			None => {
				state.scan_position = tag_name_start_position;
				state.warnings.push(crate::Warning {
					end: tag_name_end_position,
					message: crate::WarningMessage::InvalidTagSyntax,
					start: state.scan_position,
				});
			}
			Some(tag_end_position) => {
				let tag_end_position =
					tag_name_end_position + tag_end_position + 1;
				match tag_class {
					crate::TagClass::ExtensionTag => {
						if state.get_byte(tag_end_position - 2) == Some(b'/') {
							state.flush(start_position);
							state.flushed_position = tag_end_position;
							state.scan_position = state.flushed_position;
							state.nodes.push(crate::Node::Tag {
								content_end: tag_end_position,
								content_start: tag_end_position,
								end: tag_end_position,
								name: tag_name,
								nodes: vec![],
								start: start_position,
							});
						} else {
							match &tag_name as _ {
								// Fork: `pre` and `includeonly` hold text, not
								// wikitext, as `nowiki` does.
								"math" | "nowiki" | "pre" | "includeonly" => {
									// Fork: with no end tag after it, the whole
									// start tag is text, found by a search that
									// goes on from the last, so `<nowiki>`
									// repeated stays linear.
									if state.has_end_tag(
										&tag_name,
										tag_end_position,
									) {
										parse_plain_text_tag(
											state,
											start_position,
											tag_end_position,
											&tag_name,
										);
									} else {
										state.warnings.push(crate::Warning {
											end: tag_end_position,
											message: crate::WarningMessage::MissingEndTagRewinding,
											start: start_position,
										});
										state.scan_position = tag_end_position;
									}
								}
								_ => {
									let type_ = crate::OpenNodeType::Tag {
										name: tag_name,
										content_start: tag_end_position,
									};
									// Fork: as MediaWiki reads an extension tag,
									// its content runs to the first end tag of
									// its name, so one of the same name inside it
									// is text; and a start tag with no end tag
									// after it is text, found once by a search
									// that goes on from the last.
									let name = match &type_ {
										crate::OpenNodeType::Tag {
											name,
											..
										} => name.to_string(),
										_ => unreachable!(),
									};
									let nested = state.stack.iter().any(|n| {
										matches!(
											&n.type_,
											crate::OpenNodeType::Tag { name: open, .. }
												if *open == name
										)
									});
									if nested
										|| !state.has_end_tag(
											&name,
											tag_end_position,
										) {
										// The whole start tag is text, and the
										// scan goes on after it.
										state.warnings.push(crate::Warning {
											end: tag_end_position,
											message: crate::WarningMessage::MissingEndTagRewinding,
											start: start_position,
										});
										state.scan_position = tag_end_position;
									} else {
										state.push_open_node(
											type_,
											tag_end_position,
										);
									}
								}
							}
						}
					}
					crate::TagClass::Tag => {
						state.flush(start_position);
						state.flushed_position = tag_end_position;
						state.scan_position = state.flushed_position;
						state.nodes.push(crate::Node::StartTag {
							end: tag_end_position,
							name: tag_name,
							start: start_position,
						});
					}
				}
			}
		},
	}
}

fn parse_plain_text_tag<'a>(
	state: &mut crate::State<'a>,
	position_before_start_tag: usize,
	position_after_start_tag: usize,
	start_tag_name: &crate::Cow<'a, str>,
) {
	loop {
		match state.get_byte(state.scan_position) {
			None => {
				state.scan_position = position_before_start_tag + 1;
				state.warnings.push(crate::Warning {
					end: position_after_start_tag,
					message: crate::WarningMessage::MissingEndTagRewinding,
					start: position_before_start_tag,
				});
				break;
			}
			Some(b'<') => {
				if state.get_byte(state.scan_position + 1) == Some(b'/')
					&& parse_plain_text_end_tag(
						state,
						position_before_start_tag,
						position_after_start_tag,
						&start_tag_name,
					) {
					break;
				}
			}
			_ => {}
		}
		state.scan_position += 1;
	}
}

fn parse_plain_text_end_tag<'a>(
	state: &mut crate::State<'a>,
	position_before_start_tag: usize,
	position_after_start_tag: usize,
	start_tag_name: &crate::Cow<'a, str>,
) -> bool {
	let position_before_end_tag = state.scan_position;
	let position_before_end_tag_name = state.scan_position + 2;
	let mut position_after_end_tag_name = position_before_end_tag_name;
	let position_after_end_tag = loop {
		match state.get_byte(position_after_end_tag_name) {
			None | Some(b'/') | Some(b'<') => return false,
			Some(b'\t') | Some(b'\n') | Some(b' ') => {
				let position_after_end_tag = state
					.skip_whitespace_forwards(position_after_end_tag_name + 1);
				match state.get_byte(position_after_end_tag) {
					Some(b'>') => break position_after_end_tag,
					_ => return false,
				}
			}
			Some(b'>') => break position_after_end_tag_name,
			_ => position_after_end_tag_name += 1,
		}
	} + 1;
	let end_tag_name = &state.wiki_text
		[position_before_end_tag_name..position_after_end_tag_name];
	let end_tag_name =
		if end_tag_name.as_bytes().iter().all(u8::is_ascii_lowercase) {
			crate::Cow::Borrowed(end_tag_name)
		} else {
			end_tag_name.to_ascii_lowercase().into()
		};
	if *start_tag_name == end_tag_name {
		// Fork: the text before the start tag is its own node; without this it
		// was lost.
		state.flush(position_before_start_tag);
		let nodes = if position_after_start_tag < position_before_end_tag {
			vec![crate::Node::Text {
				end: position_before_end_tag,
				start: position_after_start_tag,
				value: &state.wiki_text
					[position_after_start_tag..position_before_end_tag],
			}]
		} else {
			vec![]
		};
		state.flushed_position = position_after_end_tag;
		state.scan_position = position_after_end_tag;
		state.nodes.push(crate::Node::Tag {
			content_end: position_before_end_tag,
			content_start: position_after_start_tag,
			end: position_after_end_tag,
			name: end_tag_name,
			nodes,
			start: position_before_start_tag,
		});
		return true;
	}
	let mut found = false;
	for open_node in &state.stack {
		if let crate::OpenNodeType::Tag { name, .. } = &open_node.type_ {
			if name == &end_tag_name {
				found = true;
				break;
			}
		}
	}
	if found {
		state.warnings.push(crate::Warning {
			end: position_before_end_tag,
			message: crate::WarningMessage::MissingEndTagRewinding,
			start: position_before_start_tag,
		});
		state.scan_position = position_before_start_tag + 1;
	}
	found
}
