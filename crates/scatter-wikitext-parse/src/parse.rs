// Copyright 2019 Fredrik Portström <https://portstrom.com>
// This is free software distributed under the terms specified in
// the file LICENSE at the top-level directory of this distribution.

/// Get's returned if the parsing received an unrecoverable error.
///
/// The parse stopped at a limit it was given: a time limit or a step budget.
#[derive(Debug)]
#[non_exhaustive]
pub enum ParseError<'a> {
	/// The parsing took too long and was aborted.
	TimedOut {
		/// The time the parsing took before it was aborted.
		execution_time: std::time::Duration,
		/// The output that was generated before the parsing was aborted.
		output: crate::Output<'a>,
	},
	/// Fork: the parsing took more steps than its budget and was aborted. A
	/// step is one turn of the parser's main loop, so the same text and
	/// configuration stop at the same place on every machine.
	BudgetExhausted {
		/// The budget that was exhausted.
		steps: u64,
		/// The output that was generated before the parsing was aborted.
		output: crate::Output<'a>,
	},
}

/// The limits a parse runs under. Zero means no limit.
#[derive(Clone, Copy)]
pub struct Limits {
	pub max_duration: std::time::Duration,
	pub max_steps: u64,
}

#[must_use]
pub fn parse<'a>(
	configuration: &crate::Configuration,
	wiki_text: &'a str,
	limits: Limits,
) -> Result<crate::Output<'a>, ParseError<'a>> {
	let max_duration = limits.max_duration;
	let mut state = crate::State {
		failed_at_end: Default::default(),
		flushed_position: 0,
		nodes: vec![],
		scan_position: 0,
		stack: vec![],
		warnings: vec![],
		wiki_text,
	};
	// parse redirects and warn about repeated newlines
	{
		let mut has_line_break = false;
		let mut position = 0;
		loop {
			match state.get_byte(position) {
				Some(b'\n') => {
					if has_line_break {
						state.warnings.push(crate::Warning {
							end: position + 1,
							message: crate::WarningMessage::RepeatedEmptyLine,
							start: position,
						});
					}
					has_line_break = true;
					position += 1;
					state.flushed_position = position;
					state.scan_position = position;
				}
				Some(b' ') => position += 1,
				Some(b'#') => {
					crate::redirect::parse_redirect(
						&mut state,
						configuration,
						position,
					);
					break;
				}
				_ => break,
			}
		}
	}
	let mut loop_counter = 0;
	let mut steps: u64 = 0;
	// Fork: read the clock only when a timeout was asked for, so that
	// `parse_without_timeout` runs on targets without one (wasm32-unknown-unknown).
	let start_time = if max_duration.is_zero() {
		None
	} else {
		Some(std::time::Instant::now())
	};

	crate::line::parse_beginning_of_line(&mut state, None);
	loop {
		match state.get_byte(state.scan_position) {
			None => {
				crate::line::parse_end_of_line(&mut state);
				if state.scan_position < state.wiki_text.len() {
					continue;
				}

				// rewind until we find a text node
				if let Some(crate::OpenNode {
					nodes,
					start,
					type_,
				}) = state.stack.pop()
				{
					state
						.failed_at_end
						.insert((start, std::mem::discriminant(&type_)));
					state.warnings.push(crate::Warning {
						end: state.scan_position,
						message: crate::WarningMessage::MissingEndTagRewinding,
						start,
					});
					state.rewind(nodes, start);
				} else {
					break;
				}
			}
			// invalid characters
			Some(0..=8 | 11..=31 | 127) => {
				state.warnings.push(crate::Warning {
					end: state.scan_position + 1,
					message: crate::WarningMessage::InvalidCharacter,
					start: state.scan_position,
				});
				state.scan_position += 1;
			}
			Some(b'\n') => {
				crate::line::parse_end_of_line(&mut state);
			}
			Some(b'!')
				if state.get_byte(state.scan_position + 1) == Some(b'!')
					&& match state.stack.last() {
						Some(crate::OpenNode {
							type_: crate::OpenNodeType::Table(..),
							..
						}) => true,
						_ => false,
					} =>
			{
				crate::table::parse_heading_cell(&mut state);
			}
			Some(b'&') => crate::character_entity::parse_character_entity(
				&mut state,
				configuration,
			),
			Some(b'\'') => {
				if state.get_byte(state.scan_position + 1) == Some(b'\'') {
					crate::bold_italic::parse_bold_italic(&mut state);
				} else {
					state.scan_position += 1;
				}
			}
			Some(b'<') => match state.get_byte(state.scan_position + 1) {
				Some(b'!')
					if state.get_byte(state.scan_position + 2)
						== Some(b'-') && state
						.get_byte(state.scan_position + 3)
						== Some(b'-') =>
				{
					crate::comment::parse_comment(&mut state)
				}
				Some(b'/') => {
					crate::tag::parse_end_tag(&mut state, configuration)
				}
				_ => crate::tag::parse_start_tag(&mut state, configuration),
			},
			Some(b'=') => {
				crate::template::parse_parameter_name_end(&mut state);
			}
			Some(b'[') => {
				if state.get_byte(state.scan_position + 1) == Some(b'[') {
					crate::link::parse_link_start(&mut state, configuration);
				} else {
					crate::external_link::parse_external_link_start(
						&mut state,
						configuration,
					);
				}
			}
			Some(b']') => match state.stack.pop() {
				None => state.scan_position += 1,
				Some(crate::OpenNode {
					nodes,
					start,
					type_: crate::OpenNodeType::ExternalLink,
				}) => {
					crate::external_link::parse_external_link_end(
						&mut state, start, nodes,
					);
				}
				Some(crate::OpenNode {
					nodes,
					start,
					type_: crate::OpenNodeType::Link { namespace, target },
				}) => {
					if state.get_byte(state.scan_position + 1) == Some(b']') {
						crate::link::parse_link_end(
							&mut state,
							&configuration,
							start,
							nodes,
							namespace,
							target,
						);
					} else {
						state.scan_position += 1;
						state.stack.push(crate::OpenNode {
							nodes,
							start,
							type_: crate::OpenNodeType::Link {
								namespace,
								target,
							},
						});
					}
				}
				Some(open_node) => {
					state.scan_position += 1;
					state.stack.push(open_node);
				}
			},
			Some(b'_') => {
				if state.get_byte(state.scan_position + 1) == Some(b'_') {
					crate::magic_word::parse_magic_word(
						&mut state,
						configuration,
					);
				} else {
					state.scan_position += 1;
				}
			}
			Some(b'{') => {
				if state.get_byte(state.scan_position + 1) == Some(b'{') {
					crate::template::parse_template_start(&mut state);
				} else {
					state.scan_position += 1;
				}
			}
			Some(b'|') => match state.stack.last_mut() {
				Some(crate::OpenNode {
					type_: crate::OpenNodeType::Parameter { default: None, .. },
					..
				}) => {
					crate::template::parse_parameter_separator(&mut state);
				}
				Some(crate::OpenNode {
					type_: crate::OpenNodeType::Table(..),
					..
				}) => {
					crate::table::parse_inline_token(&mut state);
				}
				Some(crate::OpenNode {
					type_: crate::OpenNodeType::Template { .. },
					..
				}) => {
					crate::template::parse_template_separator(&mut state);
				}
				_ => state.scan_position += 1,
			},
			Some(b'}') => {
				if state.get_byte(state.scan_position + 1) == Some(b'}') {
					crate::template::parse_template_end(&mut state);
				} else {
					state.scan_position += 1;
				}
			}
			_ => {
				state.scan_position += 1;
			}
		}

		if loop_counter == 10_000 {
			loop_counter = 0;
			let elapsed = start_time.map(|t| t.elapsed());
			if elapsed.map_or(false, |e| e > max_duration) {
				// Fork: the scan may stop inside a character; the output so far
				// ends at the character boundary before it.
				let mut stop = state.scan_position.min(wiki_text.len());
				while !wiki_text.is_char_boundary(stop) {
					stop -= 1;
				}
				state.flush(stop);

				return Err(ParseError::TimedOut {
					execution_time: elapsed.unwrap_or_default(),
					output: crate::Output {
						nodes: state.nodes,
						warnings: state.warnings,
						steps,
					},
				});
			}
		}

		loop_counter += 1;
		// Fork: the step budget, counted on every target and independent of
		// the clock.
		steps += 1;
		if limits.max_steps != 0 && steps > limits.max_steps {
			// Fork: the scan may stop inside a character; the output so far
			// ends at the character boundary before it.
			let mut stop = state.scan_position.min(wiki_text.len());
			while !wiki_text.is_char_boundary(stop) {
				stop -= 1;
			}
			state.flush(stop);
			return Err(ParseError::BudgetExhausted {
				steps: limits.max_steps,
				output: crate::Output {
					nodes: state.nodes,
					warnings: state.warnings,
					steps,
				},
			});
		}
	}

	let end_position = state.skip_whitespace_backwards(wiki_text.len());
	state.flush(end_position);
	Ok(crate::Output {
		nodes: state.nodes,
		warnings: state.warnings,
		steps,
	})
}
